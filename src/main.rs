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
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};
extern crate glium;
// Use the re-exported winit dependency to avoid version mismatches.
// Requires the `simple_window_builder` feature.
use crate::memory::{InterruptSource, StatMode};
use glium::winit::{self, platform::x11::EventLoopBuilderExtX11};
//use tracing_appender::non_blocking;
//use tracing_appender::{non_blocking::WorkerGuard, rolling};

const SCALE_FACTOR: u32 = 3;
// real GB screnn is 160×144, linearly scale it up linearly;
const DISPAY_HEIGHT: u32 = 180 * SCALE_FACTOR;
const DISPLAY_WIDTH: u32 = 144 * SCALE_FACTOR;

#[derive(Debug, Parser)]
struct CliArg {
    cartridge: PathBuf,
}

fn setup_tracing() /* -> WorkerGuard */
{
    // Layer 1: log INFO+ to terminal
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .compact()
        .with_ansi(true)
        .with_line_number(true)
        .with_filter(EnvFilter::new("info"));

    // Layer 2: log TRACE+ to file
    //let file_appender = rolling::minutely("logs", "emu.log");
    //let (nb_file, guard_file) = non_blocking(file_appender);

    //let file_layer = tracing_subscriber::fmt::layer()
    //    .with_writer(nb_file)
    //    .compact()
    //    .with_ansi(false)
    //    .with_line_number(true)
    //    .with_filter(EnvFilter::new("trace")); // everything

    tracing_subscriber::registry()
        .with(stdout_layer)
        //    .with(file_layer)
        .init();

    //guard_file
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

fn main() -> Result<()> {
    setup_tracing();
    let cli_args = CliArg::try_parse()?;

    // 1. The **winit::EventLoop** for handling events.
    let event_loop = winit::event_loop::EventLoop::builder().with_x11().build()?;

    // real GB screnn is 160×144, linearly scale it up
    // 2. Create a glutin context and glium Display
    let (_window, _display) = glium::backend::glutin::SimpleWindowBuilder::new()
        .with_inner_size(DISPLAY_WIDTH, DISPAY_HEIGHT)
        .with_title("crabboy")
        .build(&event_loop);

    let boot_room: Vec<u8> = std::fs::read("data/boot.gb")?;

    let cartridge = Cartridge::new(&cli_args.cartridge)?;

    let mut memory = Memory::new(cartridge, boot_room)?;

    let mut cpu = Cpu::default();
    let mut ppu = Ppu::default();

    loop {
        //let mut guard = memory.lock().expect("Failed to lock memory");
        let num_cycles = cpu.step(&mut memory)?;
        memory.registers.inc_timer(num_cycles);

        let prev_ly = memory.registers.ly;

        ppu.tick(&mut memory.registers, num_cycles);

        if prev_ly != 144 && memory.registers.ly == 144 {
            tracing::debug!("reached vblank");
        }

        for interrupt in memory.registers.get_pending_interrupts() {
            let num_cycles = cpu.service_interrupt(&interrupt, &mut memory)?;
            memory.registers.inc_timer(num_cycles);
            ppu.tick(&mut memory.registers, num_cycles);
        }
    }
}
