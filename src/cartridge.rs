use anyhow::Result;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;

pub struct Cartridge {
    pub data: Vec<u8>,
    pub header: Header,
}

impl Debug for Cartridge {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cartridge {{header={:?}}}", self.header)
    }
}

impl Cartridge {
    #[tracing::instrument(err)]
    pub fn new(path: &PathBuf) -> Result<Self> {
        let data: Vec<u8> = std::fs::read(path)?;
        let data_slice = data.as_slice();

        // Each cartridge contains a header, located at the address range $0100—$014F.

        let header_slice = &data_slice[0..=0x014F];
        let header = Header::new(header_slice)?;

        Ok(Cartridge { data, header })
    }
}

/// https://gbdev.io/pandocs/The_Cartridge_Header.html
///
/// Each cartridge contains a header, located at the address range $0100—$014F.
/// 0100-0103 — Entry point
/// After displaying the Nintendo logo, the built-in boot ROM jumps to the address $0100,
/// which should then jump to the actual main program in the cartridge. Most commercial games fill this 4-byte area with a nop instruction followed by a jp $0150.
///
/// 0104-0133 — Nintendo logo
/// This area contains a bitmap image that is displayed when the Game Boy is powered on. It must match the following (hexadecimal) dump, otherwise the boot ROM won’t allow the game to run:
/// CE ED 66 66 CC 0D 00 0B 03 73 00 83 00 0C 00 0D
/// 00 08 11 1F 88 89 00 0E DC CC 6E E6 DD DD D9 99
/// BB BB 67 63 6E 0E EC CC DD DC 99 9F BB B9 33 3E
///
/// 0134-0143 — Title
///
/// 0143 — CGB flag
/// Typical values are:
///
///     - $80 The game supports CGB enhancements, but is backwards compatible with monochrome Game Boys
///     - $C0 The game works on CGB only (the hardware ignores bit 6, so this really functions the same as $80)
///
/// 0146 — SGB flag
/// This byte specifies whether the game supports SGB functions. The SGB will ignore any command packets if this byte is set to a value other than $03 (typically $00).
///
/// 0147 — Cartridge type
///
/// 0148 — ROM size
///
/// 0149 — RAM size
///
/// 014D — Header checksum
/// This byte contains an 8-bit checksum computed from the cartridge header bytes $0134–014C. The boot ROM computes the checksum as follows:
///
/// uint8_t checksum = 0;
/// for (uint16_t address = 0x0134; address <= 0x014C; address++) {
///     checksum = checksum - rom[address] - 1;
/// }
/// The boot ROM verifies this checksum. If the byte at $014D does not match the lower 8 bits of checksum, the boot ROM will lock up and the program in the cartridge won’t run.
///
/// 014E-014F — Global checksum
/// These bytes contain a 16-bit (big-endian) checksum simply computed as the sum of all the bytes of the cartridge ROM (except these two checksum bytes).
#[derive(Debug)]
pub struct Header {
    pub title: String,
    pub cartridge_type: CartridgeType,
    pub rom_size: usize,
    pub ram_size: usize,
    pub checksum: u8,
}

impl Header {
    #[tracing::instrument(err, skip(data))]
    fn new(data: &[u8]) -> Result<Self> {
        let logo = &data[0x104..=0x133];

        anyhow::ensure!(
            logo == expected_logo(),
            "Unexpected logo.\nexpected={:?}\nfound={:?}",
            expected_logo(),
            logo
        );

        let title = &data[0x134..=0x143];

        let title: Vec<u8> = title
            .iter()
            .cloned()
            .filter(|i| -> bool { *i != 0 })
            .collect();

        let title = std::str::from_utf8(title.as_slice())?;
        let title = title.trim().to_string();

        tracing::info!("title={:?}", title);

        let gcb_flag = &data[0x143];

        // Typical values are:
        //
        // Value	Meaning
        // $80	The game supports CGB enhancements, but is backwards compatible with monochrome Game Boys
        // $C0	The game works on CGB only (the hardware ignores bit 6, so this really functions the same as $80)

        tracing::info!("GCB flag: {:#x}", gcb_flag);
        anyhow::ensure!(*gcb_flag != 0xc0, "GCB mode not implemented");

        let sgb_flag = &data[0x146];

        tracing::info!("SGB flag: {:#x}", sgb_flag);

        let cartridge_type = &data[0x147];
        let cartridge_type: CartridgeType = (*cartridge_type).try_into()?;

        tracing::info!("cartridge_type = {:?}", cartridge_type);

        anyhow::ensure!(cartridge_type == CartridgeType::RomOnly);

        let rom_size = data[0x148] as usize;
        // This byte indicates how much ROM is present on the cartridge.
        // In most cases, the ROM size is given by 32 KiB × (1 << <value>):
        let rom_size = (32 * 1024) * (1 << rom_size);
        tracing::info!("ROM size: {:#x}", rom_size);

        let ram_size = data[0x149] as usize;

        tracing::info!("RAM size: {:#x}", ram_size);

        anyhow::ensure!(ram_size == 0, "RAM not implemented");

        let mut checksum: u8 = 0;
        for v in &data[0x0134..=0x014C] {
            checksum = checksum.wrapping_sub(*v).wrapping_sub(1);
        }

        let header = Header {
            title,
            cartridge_type,
            rom_size,
            checksum,
            ram_size,
        };
        Ok(header)
    }
}

#[derive(Debug, PartialEq)]
pub enum CartridgeType {
    RomOnly,
    Mbc3RamBattery,
}

impl TryFrom<u8> for CartridgeType {
    type Error = anyhow::Error;

    /// Code   Type
    /// $00    ROM ONLY
    /// $01    MBC1
    /// $02    MBC1+RAM
    /// $03    MBC1+RAM+BATTERY
    /// $05    MBC2
    /// $06    MBC2+BATTERY
    /// $08    ROM+RAM 9
    /// $09    ROM+RAM+BATTERY 9
    /// $0B    MMM01
    /// $0C    MMM01+RAM
    /// $0D    MMM01+RAM+BATTERY
    /// $0F    MBC3+TIMER+BATTERY
    /// $10    MBC3+TIMER+RAM+BATTERY 10
    /// $11    MBC3
    /// $12    MBC3+RAM 10
    /// $13    MBC3+RAM+BATTERY 10
    /// $19    MBC5
    /// $1A    MBC5+RAM
    /// $1B    MBC5+RAM+BATTERY
    /// $1C    MBC5+RUMBLE
    /// $1D    MBC5+RUMBLE+RAM
    /// $1E    MBC5+RUMBLE+RAM+BATTERY
    /// $20    MBC6
    /// $22    MBC7+SENSOR+RUMBLE+RAM+BATTERY
    /// $FC    POCKET CAMERA
    /// $FD    BANDAI TAMA5
    /// $FE    HuC3
    /// $FF    HuC1+RAM+BATTERY
    fn try_from(value: u8) -> Result<Self> {
        match value {
            0x0 => Ok(CartridgeType::RomOnly),
            0x13 => Ok(CartridgeType::Mbc3RamBattery),
            _ => Err(anyhow::anyhow!("Unsupported cartridge type: {:#x}", value)),
        }
    }
}

fn expected_logo() -> Vec<u8> {
    vec![
        0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00,
        0x0D, 0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD,
        0xD9, 0x99, 0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB,
        0xB9, 0x33, 0x3E,
    ]
}
