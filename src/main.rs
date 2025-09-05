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
use glium::winit::{self, platform::x11::EventLoopBuilderExtX11};
use tracing_appender::non_blocking;
use tracing_appender::{non_blocking::WorkerGuard, rolling};

const SCALE_FACTOR: u32 = 3;
// real GB screnn is 160×144, linearly scale it up linearly;
const DISPAY_HEIGHT: u32 = 180 * SCALE_FACTOR;
const DISPLAY_WIDTH: u32 = 144 * SCALE_FACTOR;

#[derive(Debug, Parser)]
struct CliArg {
    cartridge: PathBuf,
}

fn setup_tracing() -> WorkerGuard {
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

#[derive(Debug, Default)]
struct Ppu {
    // T-cycles
    counter: u16,
}

impl Ppu {
    pub fn tick(&mut self, memory: &mut Memory, n_cycles: u8) {
        self.counter += n_cycles as u16;

        if self.counter >= 114 * 4 {
            self.counter = 0;
            memory.registers.ly += 1;

            // Subtract 114, increment LY.
            // If LY becomes 144, enter Mode 1 (VBlank), request VBlank interrupt, and (optionally) mark “frame ready.”
            // If LY becomes 154 → wrap to 0, enter Mode 2, and start a new frame.
            // Update STAT coincidence (LYC=LY) and fire STAT if enabled.
            if memory.registers.ly == 144 {
                memory.registers.stat.mode = memory::StatMode::Vblank;
                memory.registers.if_.vblank = true;
            } else if memory.registers.ly == 154 {
                tracing::debug!("reset LY");
                memory.registers.ly = 0;
                memory.registers.stat.mode = memory::StatMode::OamSearch;
                // TODO: start new
            } else {

                // Within the line (LY 0–143), switch modes at these cumulative thresholds:
                // 0 → 20 M: enter Mode 2 at line start (unless LCD off).
                // 20 → 63 M: switch to Mode 3 at 20 M.
                // 63 → 114 M: switch to Mode 0 at 63 M (if you’re using the fixed 43 M Mode 3).
                // On each mode switch, check STAT bits and trigger STAT interrupt if that source is enabled.

                // In VBlank lines 144–153, remain in Mode 1 for the entire 114 M; just roll the line counter as usual.
            }
        }
    }
}

fn main() -> Result<()> {
    let _guards = setup_tracing();
    let cli_args = CliArg::try_parse()?;

    // 1. The **winit::EventLoop** for handling events.
    let event_loop = winit::event_loop::EventLoop::builder()
        .with_x11()
        .build()
        .unwrap();

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

        ppu.tick(&mut memory, num_cycles);

        if prev_ly != 144 && memory.registers.ly == 144 {
            tracing::debug!("reached vblank");
        }

        for interrupt in memory.registers.get_pending_interrupts() {
            let num_cycles = cpu.service_interrupt(&interrupt, &mut memory)?;
            memory.registers.inc_timer(num_cycles);
            ppu.tick(&mut memory, num_cycles);
        }
    }
}
