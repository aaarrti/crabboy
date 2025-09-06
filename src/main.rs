mod cartridge;
mod cpu;
mod memory;
mod ppu;
mod util;

use crate::cartridge::Cartridge;
use crate::ppu::Ppu;
use clap::Parser;
use cpu::Cpu;
use memory::Memory;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

const SCALE_FACTOR: u32 = 3;
// real GB screnn is 160×144, linearly scale it up linearly;
const DISPLAY_HEIGHT: u32 = 160;
const DISPLAY_WIDTH: u32 = 144;

static HALT_REQ: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Parser)]
struct CliArg {
    #[arg(short, long)]
    cartridge: PathBuf,
    #[arg(short, long, default_value_t = false)]
    debug: bool,
}

fn setup_tracing() {
    // Layer 1: log INFO+ to terminal
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout)
        .compact()
        .with_ansi(true)
        .with_line_number(true)
        .with_filter(EnvFilter::new("info"));
    tracing_subscriber::registry().with(stdout_layer).init();
}

fn install_sigint_handler() {
    // If you prefer not to use a static, capture an Arc<AtomicBool> instead.
    ctrlc::set_handler(|| {
        tracing::info!("SIGINT received");
        HALT_REQ.store(true, Ordering::SeqCst);
    })
    .unwrap();
}

fn main() {
    let cli_args = CliArg::try_parse().unwrap();
    setup_tracing();
    let boot_room: Vec<u8> = std::fs::read("data/boot.gb").unwrap();
    let cartridge = Cartridge::new(&cli_args.cartridge);
    let mut memory = Memory::new(cartridge, boot_room);
    let mut cpu = Cpu::default();
    let mut ppu = Ppu::new();
    install_sigint_handler();

    loop {
        if HALT_REQ.load(Ordering::SeqCst) {
            tracing::info!("CPU={:?}, memory.registers=\n{:?}", cpu, memory.registers);
            break;
        }
        let num_cycles = cpu.step(&mut memory);
        memory.registers.inc_timer(num_cycles);

        ppu.tick(&mut memory.registers, num_cycles);

        if let Some(interrupt) = memory.registers.get_pending_interrupt() {
            let num_cycles = cpu.service_interrupt(&interrupt, &mut memory);
            memory.registers.inc_timer(num_cycles);
        }
        ppu.draw_frame(&memory);
    }
}
