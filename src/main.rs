mod cartridge;
mod cpu;
mod memory;
mod ppu;
mod util;

use crate::ppu::Ppu;
use anyhow;
use clap::Parser;
use cpu::Cpu;
use memory::Memory;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use tracing_appender::non_blocking;
use tracing_appender::{non_blocking::WorkerGuard, rolling};

const SCALE_FACTOR: u32 = 3;
// real GB screnn is 160×144, linearly scale it up linearly;
const DISPLAY_HEIGHT: u32 = 160;
const DISPLAY_WIDTH: u32 = 144;

static HALT_REQ: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Parser)]
struct CliArg {
    #[arg(short, long, required = true)]
    rom_path: PathBuf,
    #[arg(short, long, default_value_t = false)]
    debug: bool,
    #[arg(short, long)]
    boot_rom_path: Option<PathBuf>,
}

fn setup_tracing(enable_debug_logging_file: bool) -> Option<WorkerGuard> {
    // Layer 1: log INFO+ to terminal
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .compact()
        .with_ansi(true)
        .with_line_number(true)
        .with_filter(EnvFilter::new("info"));

    if !enable_debug_logging_file {
        return None;
    }
    // Layer 2: log TRACE+ to file
    let file_appender = rolling::minutely("logs", "emu.log");
    let (nb_file, guard_file) = non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(nb_file)
        .compact()
        .with_line_number(true)
        .with_filter(EnvFilter::new("trace")); // everything

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    Some(guard_file)
}

fn install_sigint_handler() -> anyhow::Result<()> {
    // If you prefer not to use a static, capture an Arc<AtomicBool> instead.
    ctrlc::set_handler(|| {
        tracing::info!("SIGINT received");
        HALT_REQ.store(true, Ordering::SeqCst);
    })?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli_args = CliArg::try_parse()?;

    let _guard = setup_tracing(cli_args.debug);
    let mut memory = Memory::new(&cli_args.rom_path);
    let mut cpu = Cpu::default();
    let mut ppu = Ppu::new();
    install_sigint_handler()?;

    loop {
        if HALT_REQ.load(Ordering::SeqCst) {
            tracing::info!("CPU={:?}, memory.registers=\n{:?}", cpu, memory.registers);
            break;
        }
        //let start_time = Instant::now();
        let num_cycles = cpu.step(&mut memory);
        memory.tick(num_cycles);
        ppu.tick(&mut memory.registers, num_cycles);

        //let elapsed = start_time.elapsed();
        //let cycle_duration = memory.registers.cycle_duration(num_cycles);

        //tracing::info!("elapsed = {:?}, cycle_duration={:?}", elapsed, cycle_duration);

        if let Some(interrupt) = memory.registers.get_pending_interrupt() {
            let num_cycles = cpu.service_interrupt(&interrupt, &mut memory);
            memory.tick(num_cycles);
            ppu.tick(&mut memory.registers, num_cycles);
        }
        ppu.draw_frame(&memory);
    }
    Ok(())
}
