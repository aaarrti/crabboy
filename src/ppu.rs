use crate::memory::{InterruptSource, Memory, StatMode};
use crate::{memory, DISPLAY_HEIGHT, DISPLAY_WIDTH, SCALE_FACTOR};
use glium::glutin::surface::WindowSurface;
use glium::index::NoIndices;
use glium::winit::platform::x11::EventLoopBuilderExtX11;
use glium::winit::window::Window;
use glium::{implement_vertex, winit, Display, Program, Texture2d, VertexBuffer};

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

pub struct Ppu {
    /// Think of it as an internal counter that counts 0..455 T within one LY.
    /// It resets to 0 at the start of each new scanline (when LY increments).
    /// You use it to decide which PPU mode you’re in during that line:
    ///     0–79 → OAM search
    ///     80–251 → Pixel transfer
    ///     252–455 → HBlank
    dot_counter: u16,
    frame_ready: bool,
    texture: Texture2d,
    _window: Window,
    display: Display<WindowSurface>,
    _vbo: VertexBuffer<Vertex>,
    _indices: NoIndices,
    _program: Program,
}

impl Ppu {
    pub fn new() -> Self {
        // 1. The **winit::EventLoop** for handling events.
        let event_loop = winit::event_loop::EventLoop::builder()
            .with_x11()
            .build()
            .unwrap();

        // real GB screnn is 160×144, linearly scale it up
        // 2. Create a glutin context and glium Display
        let (_window, display) = glium::backend::glutin::SimpleWindowBuilder::new()
            .with_inner_size(DISPLAY_WIDTH, DISPLAY_HEIGHT)
            .with_title("crabboy")
            .build(&event_loop);

        let program =
            glium::Program::from_source(&display, VERTEX_SHADER, FRAGMENT_SHADER, None).unwrap();

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
        let vbo = glium::VertexBuffer::new(&display, &verts).unwrap();
        let indices = glium::index::NoIndices(glium::index::PrimitiveType::TriangleStrip);

        // ----- Create texture once -----
        let empty = vec![0u8; (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4) as usize];
        let tex = {
            let raw = glium::texture::RawImage2d::from_raw_rgba_reversed(
                &empty,
                (DISPLAY_WIDTH, DISPLAY_HEIGHT),
            );
            glium::texture::Texture2d::new(&display, raw).unwrap()
        };

        Ppu {
            dot_counter: 0,
            frame_ready: false,
            texture: tex,
            _window,
            display,
            _program: program,
            _indices: indices,
            _vbo: vbo,
        }
    }

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
                } else if registers.ly >= 154 {
                    registers.ly = 0;
                    self.dot_counter = 0;
                    registers.stat.mode = StatMode::OamSearch;
                }
            }
        }
    }

    pub fn draw_frame(&mut self, memory: &Memory) {
        if !self.frame_ready {
            return;
        }

        /*
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
        */

        self.frame_ready = false;
    }
}

/// Expand a grayscale framebuffer into RGB, tripling each pixel horizontally.
///
/// Input:  Vec<u8> of grayscale values (0..255), length = width * height
/// Output: Vec<u8> of RGB values, length = width * 3 * height * 3
fn expand_gray_to_rgba_scaled(gray: &[u8]) -> Vec<u8> {
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
