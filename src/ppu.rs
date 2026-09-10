use crate::memory::{InterruptSource, Memory, StatMode};
use crate::{memory, DISPLAY_HEIGHT, DISPLAY_WIDTH, SCALE_FACTOR};

const _VERTEX_SHADER: &str = r#"
#version 140
in vec2 position;
in vec2 tex_coords;
out vec2 v_tex;
void main() {
    v_tex = tex_coords;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const _FRAGMENT_SHADER: &str = r#"
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

pub struct Ppu {
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
    pub fn new() -> Self {
        // 1. The **winit::EventLoop** for handling events.

        // real GB screen is 160×144, linearly scale it up
        // 2. Create a glutin context and glium Display


        let _verts = vec![
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

        // ----- Create texture once -----
        let _empty =
            vec![0u8; (DISPLAY_WIDTH * DISPLAY_HEIGHT * 4 * SCALE_FACTOR * SCALE_FACTOR) as usize];
      

        Ppu {
            dot_counter: 0,
            frame_ready: false,
        }
    }

    #[tracing::instrument(skip(self, registers))]
    pub fn tick(&mut self, registers: &mut memory::Registers, n_cycles: u8) {
        // LCD off: frozen state
        if !registers.lcdc.lcd_ppu_enable {
            self.dot_counter = 0;
            registers.ly = 0;
            registers.stat.mode = StatMode::Hblank;
            return;
        }

        // Work in u16 for arithmetic headroom
        let mut dot = self.dot_counter;
        let mut ly = registers.ly as u16;
        let mut t = n_cycles;

        while t != 0 {
            // Determine current mode and next boundary (in T-cycles)
            let (mode, next_boundary) = if ly < 144 {
                // visible line: boundaries at 80, 252, 456
                if dot < 80 {
                    (StatMode::OamSearch, 80u16)
                } else if dot < 252 {
                    (StatMode::LcdTransfer, 252u16)
                } else {
                    (StatMode::Hblank, 456u16)
                }
            } else {
                // VBlank line: whole line is Mode 1, boundary at 456
                (StatMode::Vblank, 456u16)
            };

            // Update STAT mode only if it changed
            if registers.stat.mode != mode {
                registers.stat.mode = mode;
                // If you support STAT mode-entry IRQs, fire them here:
                // - entering OAM  (bit5)
                // - entering VBlank (bit4)
                // - entering HBlank (bit3)
                // Example:
                // if mode == StatMode::OamSearch && registers.stat.oam_ir_enable { registers.request_interrupt(&InterruptSource::Stat); }
                // if mode == StatMode::Vblank    && registers.stat.v_blank_ir_enable { registers.request_interrupt(&InterruptSource::Stat); }
                // if mode == StatMode::Hblank    && registers.stat.h_blank_ir_enable { registers.request_interrupt(&InterruptSource::Stat); }

                if (mode == StatMode::OamSearch && registers.stat.oam_ir_enable)
                    || (mode == StatMode::Vblank && registers.stat.v_blank_ir_enable)
                    || (mode == StatMode::Hblank && registers.stat.h_blank_ir_enable)
                {
                    registers.request_interrupt(&InterruptSource::Stat);
                }
            }

            // How many T-cycles until this boundary?
            let to_boundary = next_boundary - dot;
            let step = to_boundary.min(t as u16); // chunk we can consume now

            dot += step;
            t -= step as u8;

            // Hit boundary? Handle once, then loop continues
            if dot == next_boundary && next_boundary == 456 {
                // End of scanline
                dot = 0;
                ly = ly.wrapping_add(1);

                // Update LY & coincidence once per line
                registers.ly = (ly & 0x00FF) as u8;
                registers.update_stat_coincidence();

                if ly == 144 {
                    // Entering VBlank
                    registers.stat.mode = StatMode::Vblank;
                    registers.request_interrupt(&InterruptSource::VBlank);
                    if registers.stat.v_blank_ir_enable {
                        registers.request_interrupt(&InterruptSource::Stat);
                    }
                    // Coincidence already updated just above
                    self.frame_ready = true;
                } else if ly >= 154 {
                    // Wrap to new frame
                    ly = 0;
                    dot = 0;
                    registers.ly = 0;
                    registers.stat.mode = StatMode::OamSearch;
                    registers.update_stat_coincidence();
                }
            }
            // else: just crossed 80 or 252; loop will recompute mode next iteration
        }
        self.dot_counter = dot;
    }


    #[tracing::instrument(skip(self, memory))]
    pub fn draw_frame(&mut self, memory: &Memory) {
        if !self.frame_ready {
            return;
        }

        let fb = memory.decode_framebuffer();
        let _fb = expand_gray_to_rgba_scaled(fb.as_slice());

        // Upload to texture (top-left origin in our buffer → use *_reversed)

        // Draw
        tracing::warn!("ppu.draw_frame -> not implemented");
        self.frame_ready = false;
    }
}

/// Expand a grayscale framebuffer into RGB, tripling each pixel horizontally.
///
/// Input:  Vec<u8> of grayscale values (0..255), length = WIDTH * HEIGHT
/// Output: Vec<u8> of RGB values, length = WIDTH * 3 * HEIGHT * 3
fn expand_gray_to_rgba_scaled(gray: &[u8]) -> Vec<u8> {
    const WIDTH: usize = DISPLAY_WIDTH as usize;
    const HEIGHT: usize = DISPLAY_HEIGHT as usize;
    const SCALE: usize = SCALE_FACTOR as usize;

    let out_w = WIDTH * SCALE;
    let out_h = HEIGHT * SCALE;
    let mut out = Vec::with_capacity(out_w * out_h * 4);

    // Reusable buffer for one horizontally-scaled RGBA row.
    let mut row = Vec::with_capacity(out_w * 4);

    for y in 0..HEIGHT {
        row.clear();

        // Build one expanded row (horizontal SCALE only).
        let base = y * WIDTH;
        for x in 0..WIDTH {
            let v = gray[base + x];
            let px = [v, v, v, 255]; // (use 255 alpha; 0 made all pixels transparent)
                                     // Repeat horizontally `SCALE` times.
            for _ in 0..SCALE {
                row.extend_from_slice(&px);
            }
        }

        // Copy the row `SCALE` times to achieve vertical scaling.
        for _ in 0..SCALE {
            out.extend_from_slice(&row);
        }
    }

    out
}
