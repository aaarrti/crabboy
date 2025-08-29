mod cartridge;
mod cpu;
mod interrupt;
mod memory;
mod util;

use crate::cartridge::Cartridge;
use anyhow::Result;
use clap::Parser;
use cpu::Cpu;
use memory::Memory;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
struct CliArg {
    boot_rom: PathBuf,
    cartridge: PathBuf,
}

fn setup_tracing() -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        // .pretty()
        .compact()
        .with_env_filter(EnvFilter::from_default_env())
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_line_number(true)
        //.with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    Ok(())
}

fn main() -> Result<()> {
    setup_tracing()?;
    let cli_args = CliArg::try_parse()?;

    let boot_room: Vec<u8> = std::fs::read(&cli_args.boot_rom)?;

    let cartridge = Cartridge::new(&cli_args.cartridge)?;

    let cpu = Cpu::default();

    let memory = Memory::new(cartridge, boot_room)?;

    let memory = Arc::new(Mutex::new(memory));

    cpu.run(memory)
}
