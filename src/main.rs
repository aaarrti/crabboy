mod cartridge;
mod cpu;
mod memory;
mod util;

use crate::cartridge::Cartridge;
use anyhow::Result;
use clap::Parser;
use cpu::Cpu;
use memory::Memory;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};
extern crate glium;
// Use the re-exported winit dependency to avoid version mismatches.
// Requires the `simple_window_builder` feature.
use crate::memory::{InterruptSource, StatMode};
use glium::winit::{self, platform::x11::EventLoopBuilderExtX11};
use glium::{implement_vertex, uniform, Surface};
use tracing_appender::non_blocking;
use tracing_appender::{non_blocking::WorkerGuard, rolling};

const SCALE_FACTOR: u32 = 3;
// real GB screnn is 160×144, linearly scale it up linearly;
const DISPLAY_HEIGHT: u32 = 160 * SCALE_FACTOR;
const DISPLAY_WIDTH: u32 = 144 * SCALE_FACTOR;

static HALT_REQ: AtomicBool = AtomicBool::new(false);

const VERTEX_SHADER: &str = r#"
#version 140
in vec2 position;
in vec2 tex_coords;
out vec2 v_tex;
void main() {
    v_tex = tex_coords;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const FRAGMENT_SHADER: &str = r#"
#version 140
in vec2 v_tex;
out vec4 color;
uniform sampler2D fb;
void main() {
    color = texture(fb, v_tex);
}
"#;

#[derive(Copy, Clone)]
struct Vertex {
    position: [f32; 2],
    tex_coords: [f32; 2],
}
implement_vertex!(Vertex, position, tex_coords);

#[derive(Debug, Parser)]
struct CliArg {
    #[arg(short, long)]
    cartridge: PathBuf,
    #[arg(short, long, default_value_t = false)]
    debug: bool,
}

fn setup_debug_tracing() -> WorkerGuard {
    // Layer 1: log INFO+ to terminal
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .compact()
        .with_ansi(true)
        .with_line_number(true)
        .with_filter(EnvFilter::new("info"));

    // Layer 2: log TRACE+ to file
    let file_appender = rolling::minutely("logs", "emu.log");
    let (nb_file, guard_file) = non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(nb_file)
        .compact()
        .with_ansi(false)
        .with_line_number(true)
        .with_filter(EnvFilter::new("trace")); // everything

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    guard_file
}

fn setup_info_tracing() {
    // Layer 1: log INFO+ to terminal
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .compact()
        .with_ansi(true)
        .with_line_number(true)
        .with_filter(EnvFilter::new("info"));

    tracing_subscriber::registry().with(stdout_layer).init();
}

#[derive(Debug, Default)]
struct Ppu {
    /// Think of it as an internal counter that counts 0..455 T within one LY.
    /// It resets to 0 at the start of each new scanline (when LY increments).
    /// You use it to decide which PPU mode you’re in during that line:
    ///     0–79 → OAM search
    ///     80–251 → Pixel transfer
    ///     252–455 → HBlank
    dot_counter: u16,
    frame_ready: bool,
}

impl Ppu {
    pub fn tick(&mut self, registers: &mut memory::Registers, n_cycles: u8) {
        if !registers.lcdc.lcd_ppu_enable {
            self.dot_counter = 0;
            registers.ly = 0;
            registers.stat.mode = StatMode::Hblank;
            return;
        }

        let mut t = n_cycles;

        while t > 0 {
            t -= 1;
            self.dot_counter += 1;

            if registers.ly <= 143 {
                // visible scanlines

                match self.dot_counter {
                    0 => registers.stat.mode = StatMode::OamSearch,
                    80 => registers.stat.mode = StatMode::LcdTransfer,
                    252 => registers.stat.mode = StatMode::Hblank,
                    _ => {}
                }
            } else {
                registers.stat.mode = StatMode::Vblank;
            }

            if self.dot_counter >= 456 {
                self.dot_counter -= 456;
                registers.ly += 1;
                registers.update_stat_coincidence();

                if registers.ly == 144 {
                    registers.stat.mode = StatMode::Vblank;
                    registers.request_interrupt(&InterruptSource::VBlank);
                    if registers.stat.v_blank_ir_enable {
                        registers.request_interrupt(&InterruptSource::Stat);
                    }
                    registers.update_stat_coincidence();
                    self.frame_ready = true;
                    tracing::debug!("frame ready");
                } else if registers.ly >= 154 {
                    registers.ly = 0;
                    self.dot_counter = 0;
                    registers.stat.mode = StatMode::OamSearch;
                }
            }
            // (Optional) perform per-line housekeeping here
        }
    }
}

/// Expand a grayscale framebuffer into RGB, tripling each pixel horizontally.
///
/// Input:  Vec<u8> of grayscale values (0..255), length = width * height
/// Output: Vec<u8> of RGB values, length = width * 3 * height * 3
pub fn expand_gray_to_rgba_scaled(gray: &[u8]) -> Vec<u8> {
    let width = (DISPLAY_WIDTH / SCALE_FACTOR) as usize;
    let height = (DISPLAY_HEIGHT / SCALE_FACTOR) as usize;
    let scale = SCALE_FACTOR as usize;

    let out_w = width * scale;
    let out_h = height * scale;
    let mut out = Vec::with_capacity(out_w * out_h * 4);

    // Reusable buffer for one horizontally-scaled RGBA row.
    let mut row = Vec::with_capacity(out_w * 4);

    for y in 0..height {
        row.clear();

        // Build one expanded row (horizontal scale only).
        let base = y * width;
        for x in 0..width {
            let v = gray[base + x];
            let px = [v, v, v, 255]; // (use 255 alpha; 0 made all pixels transparent)
                                     // Repeat horizontally `scale` times.
            for _ in 0..scale {
                row.extend_from_slice(&px);
            }
        }

        // Copy the row `scale` times to achieve vertical scaling.
        for _ in 0..scale {
            out.extend_from_slice(&row);
        }
    }

    out
}

fn install_sigint_handler() -> Result<()> {
    // If you prefer not to use a static, capture an Arc<AtomicBool> instead.
    ctrlc::set_handler(|| {
        tracing::info!("SIGINT received");
        HALT_REQ.store(true, Ordering::SeqCst);
    })?;
    Ok(())
}

fn main() -> Result<()> {
    let cli_args = CliArg::try_parse()?;

    let _guard = if cli_args.debug {
        Some(setup_debug_tracing())
    } else {
        setup_info_tracing();
        None
    };

    // 1. The **winit::EventLoop** for handling events.
    let event_loop = winit::event_loop::EventLoop::builder().with_x11().build()?;

    // real GB screnn is 160×144, linearly scale it up
    // 2. Create a glutin context and glium Display
    let (_window, display) = glium::backend::glutin::SimpleWindowBuilder::new()
        .with_inner_size(DISPLAY_WIDTH, DISPLAY_HEIGHT)
        .with_title("crabboy")
        .build(&event_loop);

    let program = glium::Program::from_source(&display, VERTEX_SHADER, FRAGMENT_SHADER, None)?;

    let verts = vec![
        Vertex {
            position: [-1.0, -1.0],
            tex_coords: [0.0, 1.0],
        },
        Vertex {
            position: [-1.0, 1.0],
            tex_coords: [0.0, 0.0],
        },
        Vertex {
            position: [1.0, -1.0],
            tex_coords: [1.0, 1.0],
        },
        Vertex {
            position: [1.0, 1.0],
            tex_coords: [1.0, 0.0],
        },
    ];
    let vbo = glium::VertexBuffer::new(&display, &verts)?;
    let indices = glium::index::NoIndices(glium::index::PrimitiveType::TriangleStrip);

    // ----- Create texture once -----
    let empty = vec![0u8; (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4) as usize];
    let tex = {
        let raw = glium::texture::RawImage2d::from_raw_rgba_reversed(
            &empty,
            (DISPLAY_WIDTH, DISPLAY_HEIGHT),
        );
        glium::texture::Texture2d::new(&display, raw)?
    };

    let boot_room: Vec<u8> = std::fs::read("data/boot.gb")?;

    let cartridge = Cartridge::new(&cli_args.cartridge)?;

    let mut memory = Memory::new(cartridge, boot_room)?;

    let mut cpu = Cpu::default();
    let mut ppu = Ppu::default();

    install_sigint_handler()?;

    loop {
        if HALT_REQ.load(Ordering::SeqCst) {
            tracing::info!("CPU={:?}, memory.registers=\n{:?}", cpu, memory.registers);
            break;
        }

        //let mut guard = memory.lock().expect("Failed to lock memory");
        let num_cycles = cpu.step(&mut memory)?;
        memory.registers.inc_timer(num_cycles);

        ppu.tick(&mut memory.registers, num_cycles);

        if let Some(interrupt) = memory.registers.get_pending_interrupt() {
            let num_cycles = cpu.service_interrupt(&interrupt, &mut memory)?;
            memory.registers.inc_timer(num_cycles);
        }

        if ppu.frame_ready {
            let fb = memory.decode_framebuffer();
            let fb = expand_gray_to_rgba_scaled(fb.as_slice());

            // Upload to texture (top-left origin in our buffer → use *_reversed)
            let raw = glium::texture::RawImage2d::from_raw_rgba_reversed(
                &fb,
                (DISPLAY_WIDTH, DISPLAY_HEIGHT),
            );

            tex.write(
                glium::Rect {
                    left: 0,
                    bottom: 0,
                    width: DISPLAY_WIDTH,
                    height: DISPLAY_HEIGHT,
                },
                raw,
            );

            // Draw
            let mut frame = display.draw();
            let uniforms = uniform! {
                fb: tex.sampled().magnify_filter(glium::uniforms::MagnifySamplerFilter::Nearest)
                               .minify_filter(glium::uniforms::MinifySamplerFilter::Nearest),
            };
            frame.draw(&vbo, indices, &program, &uniforms, &Default::default())?;
            frame.finish()?;

            ppu.frame_ready = false;
        }
    }
    Ok(())
}
