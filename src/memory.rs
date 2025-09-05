use crate::cartridge::Cartridge;
use crate::util::is_nth_bit_set;
use anyhow::Result;
use std::vec;

const BOOT_ROM_END: usize = 0x00FF;
const ROM_0_END: usize = 0x3FFF;

const ROM_N_START: usize = 0x4000;
const ROM_N_END: usize = 0x7FFF;

const VRAM_START: usize = 0x8000;
const VRAM_END: usize = 0x9FFF;

const EX_RAM_START: usize = 0xA000;
const EX_RAM_END: usize = 0xBFFF;

// 2 banks together
const WRAM_START: usize = 0xC000;
const WRAM_END: usize = 0xDFFF;

const ECHO_RAM_START: usize = 0xE000;
const ECHO_RAM_END: usize = 0xFDFF;

const OAM_START: usize = 0xFE00;
const OAM_END: usize = 0xFE9F;

const NOT_USABLE_START: usize = 0xFEA0;
const NOT_USABLE_END: usize = 0xFEFF;

const IO_START: usize = 0xFF00;
const IO_END: usize = 0xFF7F;

const HRAM_START: usize = 0xFF80;
const HRAM_END: usize = 0xFFFE;

const BANK_SELECT_REGISTER: usize = 0x2000;

// ROM Bank Number (0x2000-0x3FFF)
// This determines which 16KB ROM bank is selected for the upper half of the address space (0x4000 - 0x7FFF).
// RAM Bank Select (0x0000 - 0x1FFF)
//This range of memory addresses is used to select different RAM bank numbers (if the cartridge includes RAM).

// Hardware registers
// As far as timing-sensitive values are concerned, these values are recorded at PC = $0100.
//
// Name	Address
// P1	$FF00
// SB	$FF01
// SC	$FF02
// DIV	$FF04
// TIMA	$FF05
// TMA	$FF06
// TAC	$FF07
// IF	$FF0F
// NR10	$FF10
// NR11	$FF11
// NR12	$FF12
// NR13	$FF13
// NR14	$FF14
// NR21	$FF16
// NR22	$FF17
// NR23	$FF18
// NR24	$FF19
// NR30	$FF1A
// NR31	$FF1B
// NR32	$FF1C
// NR33	$FF1D
// NR34	$FF1E
// NR41	$FF20
// NR42	$FF21
// NR43	$FF22
// NR44	$FF23
// NR50	$FF24
// NR51	$FF25
// NR52	$FF26
// LCDC	$FF40
// STAT	$FF41
// SCY	$FF42
// SCX	$FF43
// LY	$FF44
// LYC	$FF45
// DMA	$FF46
// BGP	$FF47
// OBP0	$FF48
// OBP1	$FF49
// WY	$FF4A
// WX	$FF4B
// IE	$FFFF

const P1_REG: usize = 0xFF00;
const SB_REG: usize = 0xFF01;
const SC_REG: usize = 0xFF02;
const DIV_REG: usize = 0xFF04;
const TIMA_REG: usize = 0xFF05;
const TMA_REG: usize = 0xFF06;
const TAC_REG: usize = 0xFF07;
const IF_REG: usize = 0xFF0F;

const NR_10_REG: usize = 0xFF10;

const NR_11_REG: usize = 0xFF11;

const NR_12_REG: usize = 0xFF12;
const NR_13_REG: usize = 0xFF13;
const NR_14_REG: usize = 0xFF14;
const NR_21_REG: usize = 0xFF16;
const NR_22_REG: usize = 0xFF17;
const NR_23_REG: usize = 0xFF18;
const NR_24_REG: usize = 0xFF19;
const NR_30_REG: usize = 0xFF1A;

const NR_31_REG: usize = 0xFF1B;
const NR_32_REG: usize = 0xFF1C;
const NR_33_REG: usize = 0xFF1D;
const NR_34_REG: usize = 0xFF1E;
const NR_41_REG: usize = 0xFF20;
const NR_42_REG: usize = 0xFF21;
const NR_43_REG: usize = 0xFF22;
const NR_44_REG: usize = 0xFF23;
const NR_50_REG: usize = 0xFF24;
const NR_51_REG: usize = 0xFF25;
const NR_52_REG: usize = 0xFF26;

const LCDC_REG: usize = 0xFF40;
const STAT_REG: usize = 0xFF41;
const SCY_REG: usize = 0xFF42;
const SCX_REG: usize = 0xFF43;
const LY_REG: usize = 0xFF44;
const LYC_REG: usize = 0xFF45;
const DMA_REG: usize = 0xFF46;
const BGP_REG: usize = 0xFF47;
const OBP0_REG: usize = 0xFF48;
const OBP1_REG: usize = 0xFF49;
const WY_REG: usize = 0xFF4A;
const WX_REG: usize = 0xFF4B;
const IE_REG: usize = 0xFFFF;

const TILE_DATA_START: usize = 0x8000;
const TILE_DATA_END: usize = 0x97FF;
const BG_MAP_0_START: usize = 0x9800;
const BG_MAP_0_END: usize = 0x9BFF;
const BOOT_ROM_DISABLE_REG: usize = 0xFF50;

/// Interrupt Handling
/// The IF bit corresponding to this interrupt and the IME flag are reset by the CPU. The former “acknowledges” the interrupt, while the latter prevents any further interrupts from being handled until the program re-enables them, typically by using the reti instruction.
/// The corresponding interrupt handler (see the IE and IF register descriptions above) is called by the CPU. This is a regular call, exactly like what would be performed by a call <address> instruction (the current PC is pushed onto the stack and then set to the address of the interrupt handler).
/// The following interrupt service routine is executed when control is being transferred to an interrupt handler:
///
/// Two wait states are executed (2 M-cycles pass while nothing happens; presumably the CPU is executing nops during this time).
/// The current value of the PC register is pushed onto the stack, consuming 2 more M-cycles.
/// The PC register is set to the address of the handler (one of: $40, $48, $50, $58, $60). This consumes one last M-cycle.
/// The entire process lasts 5 M-cycles.
///
/// Jump Vectors in first ROM bank
/// The following addresses are supposed to be used as jump vectors:
///
/// RST instructions: 0000, 0008, 0010, 0018, 0020, 0028, 0030, 0038
/// Interrupts: 0040, 0048, 0050, 0058, 0060
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InterruptSource {
    // This interrupt is requested every time the Game Boy enters VBlank (Mode 1).
    VBlank,
    /// https://gbdev.io/pandocs/STAT.html#ff41--stat-lcd-status
    Stat,
    /// The timer interrupt is requested every time that the timer overflows (that is, when TIMA exceeds $FF).
    Timer,
    /// The serial interrupt is requested upon completion of a serial data transfer.
    /// In other words, eight serial clock cycles after starting a transfer (by setting SC bit 7), the incoming data will be in SB and the interrupt will be requested.
    Serial,
    /// The Joypad interrupt is requested when any of P1 bits 0-3 change from High to Low. This happens when a button is pressed
    Joypad,
}

impl InterruptSource {
    /// Interrupt   IF Bit  Vector Address
    /// VBlank      Bit 0   0x0040
    /// LCD STAT    Bit 1   0x0048
    /// Timer       Bit 2   0x0050
    /// Serial      Bit 3   0x0058
    /// Joypad      Bit 4   0x0060
    pub fn jump_addres(&self) -> u16 {
        match self {
            InterruptSource::VBlank => 0x0040,
            InterruptSource::Stat => 0x0048,
            InterruptSource::Timer => 0x0050,
            InterruptSource::Serial => 0x0058,
            InterruptSource::Joypad => 0x0060,
        }
    }
}

/// The Game Boy has a 16-bit address bus, which is used to address ROM, RAM, and I/O.
///
/// 0000    3FFF    16 KiB ROM bank 00                From cartridge, usually a fixed bank
/// 4000    7FFF    16 KiB ROM Bank 01–NN             From cartridge, switchable bank via mapper (if any)
///
/// 8000    9FFF    8 KiB Video RAM (VRAM)
/// A000    BFFF    8 KiB External RAM                From cartridge, switchable bank if any
///
/// C000    CFFF    4 KiB Work RAM (WRAM)
/// D000    DFFF    4 KiB Work RAM (WRAM)
///
/// E000    FDFF    Echo RAM (mirror of C000–DDFF)    Nintendo says use of this area is prohibited.
/// FE00    FE9F    Object attribute memory (OAM)
/// FEA0    FEFF    Not Usable                        Nintendo says use of this area is prohibited.
///
/// FF00    FF7F    I/O Registers
///
/// FF80    FFFE    High RAM (HRAM)
/// FFFF    FFFF    Interrupt Enable register (IE)
pub struct Memory {
    boot_rom: [u8; BOOT_ROM_END + 1],
    rom_0: [u8; ROM_0_END + 1],
    // rom_n: Option<[u8; ROM_N_END - ROM_N_START + 1]>,
    vram: Vram,
    // ex_ram: Option<[u8; EX_RAM_END - EX_RAM_START + 1]>,
    pub registers: Registers,
    // 2 banks together
    wram: Wram,
    hram: Hram,
    pub cartridge: Cartridge,
    boot_rom_mapped: bool,
    io_: IoMem,
    oam: ObjectAttributeMemory,
}

impl Memory {
    #[tracing::instrument(err)]
    pub fn new(cartridge: Cartridge, boot_rom: Vec<u8>) -> Result<Self> {
        let mut rom_0 = [0; ROM_0_END + 1];
        let vram = Vram::new();
        let wram = Wram::new();
        let io_ = IoMem::default();

        // mmap first 16KB
        let data = cartridge.data.as_slice();
        rom_0.copy_from_slice(&data[0..=ROM_0_END]);
        // rom_n.copy_from_slice(&cartridge[ROM_N_START..ROM_N_END + 1]);

        let hram = Hram::new();
        let registers = Registers::default();

        let data = boot_rom.as_slice();
        let mut boot_rom_ = [0; BOOT_ROM_END + 1];
        boot_rom_.copy_from_slice(&data[0..=BOOT_ROM_END]);

        let memory = Memory {
            boot_rom: boot_rom_,
            rom_0,
            vram,
            wram,
            hram,
            registers,
            cartridge,
            io_,
            boot_rom_mapped: true,
            oam: ObjectAttributeMemory::default(),
        };

        Ok(memory)
    }

    #[tracing::instrument(skip(self), err)]
    pub fn read(&self, address: u16) -> Result<u8> {
        let address = address as usize;

        if self.boot_rom_mapped && address <= BOOT_ROM_END {
            return Ok(self.boot_rom[address]);
        }
        if Registers::matches(address) {
            // tracing::debug!("Read from hardware register at: {:#04x}", address);
            return self.registers.read(address);
        }

        if address <= ROM_0_END {
            let limit = self.cartridge.header.rom_size;
            anyhow::ensure!(
                address < limit,
                "Address {:#x} out of bounds for ROM size {:#x}",
                address,
                limit
            );
            return Ok(self.rom_0[address]);
        }

        if (ROM_N_START..=ROM_N_END).contains(&address) {
            // let address = address - ROM_N_START;
            // return Ok(self.rom_n[address]);
            anyhow::bail!("ROM n not implemeted")
        }

        if Vram::matches(address) {
            return Ok(self.vram.read(address));
        };

        if (EX_RAM_START..=EX_RAM_END).contains(&address) {
            anyhow::bail!("EX RAM no implemeted");
        }

        if Wram::matches(address) {
            return Ok(self.wram.read(address));
        }

        if (ECHO_RAM_START..=ECHO_RAM_END).contains(&address) {
            anyhow::bail!("Illegal read to ECHO RAM at {:#04x}", address)
        }

        if (OAM_START..=OAM_END).contains(&address) {
            anyhow::bail!("OAM read not implemted")
        }

        if (NOT_USABLE_START..=NOT_USABLE_END).contains(&address) {
            anyhow::bail!("Illegal read to NOT USABLE at {:#04x}", address)
        }

        if IoMem::matches(address) {
            return self.io_.read(address);
        }

        if Hram::matches(address) {
            return Ok(self.hram.read(address));
        }

        anyhow::bail!("Illegal address = {:#04x}", address);
    }

    #[tracing::instrument(skip(self), err)]
    pub fn write(&mut self, address: u16, value: u8) -> Result<()> {
        let address = address as usize;

        if address == BOOT_ROM_DISABLE_REG {
            // if self.boot_rom_mapped {
            if self.boot_rom_mapped && value != 0x0 {
                tracing::info!("Boot ROM done executing!");
                self.boot_rom_mapped = false;
                return Ok(());
            }
        }
        if address <= BOOT_ROM_END {
            anyhow::bail!("Illegal write to boot ROM at {:#04x}", address);
        }
        // }
        if Registers::matches(address) {
            // tracing::debug!("Write hardware register at: {:#04x}", address);
            return self.registers.write(address, value);
        }

        if address == BANK_SELECT_REGISTER {
            // thos writes are handled by MBC on cartridge
            // rom only cartridge - no bank switch, just ignore
            return Ok(());
        }

        if address == IE_REG {
            anyhow::bail!("IE REG write not implemented");
        }

        if address <= ROM_0_END {
            anyhow::bail!("Illegal rite to ROM bank 0 at {:#04x}", address)
        }

        if (ROM_N_START..=ROM_N_END).contains(&address) {
            anyhow::bail!("Illegal write to ROM bank n at {:#04x}", address)
        }

        if Vram::matches(address) {
            self.vram.write(address, value);
            return Ok(());
        }

        if (EX_RAM_START..=EX_RAM_END).contains(&address) {
            // let address = address - WRAM_START;
            // self.ex_ram[address] = value;
            // return Ok(());
            anyhow::bail!("EX RAM no implemeted");
        }

        if Wram::matches(address) {
            self.wram.write(address, value);
            return Ok(());
        }

        if (ECHO_RAM_START..=ECHO_RAM_END).contains(&address) {
            anyhow::bail!("Illegal access to ECHO RAM at {:#04x}", address)
        }

        if (OAM_START..=OAM_END).contains(&address) {
            // CPU access is blocked during Mode 2 & 3 (OAM search and drawing), but allowed in HBlank & VBlank.

            return match &self.registers.stat.mode {
                StatMode::Hblank | StatMode::Vblank => self.oam.write(address, value),

                mode => {
                    tracing::warn!("Blocking OAM write during mode = {:?}", mode);
                    Ok(())
                }
            };
        }

        if (NOT_USABLE_START..=NOT_USABLE_END).contains(&address) {
            tracing::warn!("Illegal write to NOT USABLE at {:#04x}", address);
            return Ok(());
        }

        if IoMem::matches(address) {
            return self.io_.write(address, value);
        }

        if Hram::matches(address) {
            self.hram.write(address, value);
            return Ok(());
        }

        anyhow::bail!("Illegal address = {:#04x}", address)
    }
}

/// Tile Data (0x8000–0x97FF)
/// - Stores 384 tiles, each 16 bytes (8×8 pixels × 2 bits per pixel).
/// - Two addressing modes:
/// - Unsigned mode (0x8000–0x8FFF) — tiles 0–255
/// - Signed mode (0x8800–0x97FF) — tiles -128 to 127
/// - Controlled by bit 4 of LCDC (0xFF40)
///
/// Tile Maps (BG Maps)
/// - 0x9800–0x9BFF — Tile Map 0
/// - 0x9C00–0x9FFF — Tile Map 1
///     - Each map is 32×32 tiles = 1024 bytes
///     - Each byte is a tile index (which tile to show at that screen location)
///     - Controlled by bit 3 of LCDC
///
/// VRAM Access Rules
/// - VRAM is not accessible during certain PPU modes (2 and 3: OAM scan + drawing)
/// - Writes during these periods will be ignored or cause glitches
///   0x8000   0x97FF   Tile Data (Pattern Tables)   Stores 8x8 pixel tile graphics (sprites & backgrounds)
///   0x9800   0x9BFF   BG Map 0 (Tile Map 0)        Background tile map â€” layout of tiles on screen
///   0x9C00   0x9FFF   BG Map 1 (Tile Map 1) .      Alternate background tile map
struct Vram([u8; VRAM_END - VRAM_START + 1]);

impl Vram {
    fn new() -> Self {
        Self([0; VRAM_END - VRAM_START + 1])
    }

    fn matches(address: usize) -> bool {
        (VRAM_START..=VRAM_END).contains(&address)
    }

    fn read(&self, address: usize) -> u8 {
        let address = address - VRAM_START;
        self.0[address]
    }

    fn write(&mut self, address: usize, value: u8) {
        let address = address - VRAM_START;
        self.0[address] = value;
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct SpriteAttribute {
    y: u8,
    x: u8,
    tile: u8,
    attr: u8,
}

/// In the Game Boy, OAM (Object Attribute Memory) is a 160-byte table at 0xFE00–0xFE9F.
/// It holds all the sprite attribute data the PPU needs to draw sprites. There are 40 sprite entries, each 4 bytes long:
///
/// Layout per sprite (4 bytes)
/// Offset  Name    Meaning
/// +0  Y position  Sprite’s vertical position on screen = (value − 16). Values 0–255 wrap.
/// +1  X position  Sprite’s horizontal position = (value − 8). Values 0–255 wrap.
/// +2  Tile index  Which 8×8 tile to use (from tile data in VRAM). Interpretation depends on LCDC (8×8 vs 8×16 sprites).
/// +3  Attributes  Flags controlling rendering (see below).
#[derive(Debug)]
struct ObjectAttributeMemory {
    sprites: [SpriteAttribute; 40usize],
}

impl Default for ObjectAttributeMemory {
    fn default() -> Self {
        let sprites = [SpriteAttribute::default(); 40];

        ObjectAttributeMemory { sprites }
    }
}

impl ObjectAttributeMemory {
    fn write(&mut self, address: usize, value: u8) -> Result<()> {
        // index = addr - 0xFE00   // 0–159
        // sprite_id = index / 4   // 0–39
        // field = index % 4       // 0..=3

        let index = address - OAM_START;
        let sprite_id = index / 4;

        let sprite = &mut self.sprites[sprite_id];

        match index % 4 {
            0 => {
                sprite.y = value;
            }

            1 => {
                sprite.x = value;
            }

            2 => {
                sprite.tile = value;
            }

            3 => {
                sprite.attr = value;
            }

            _ => anyhow::bail!("this is unexpected"),
        }

        Ok(())
    }
}

struct Hram([u8; HRAM_END - HRAM_START + 1]);

impl Hram {
    fn new() -> Self {
        Self([0; HRAM_END - HRAM_START + 1])
    }

    fn matches(address: usize) -> bool {
        (HRAM_START..=HRAM_END).contains(&address)
    }

    fn read(&self, address: usize) -> u8 {
        let address = address - HRAM_START;
        self.0[address]
    }

    fn write(&mut self, address: usize, value: u8) {
        let address = address - HRAM_START;
        self.0[address] = value;
    }
}

struct Wram([u8; WRAM_END - WRAM_START + 1]);

impl Wram {
    fn new() -> Self {
        Self([0; WRAM_END - WRAM_START + 1])
    }

    fn matches(address: usize) -> bool {
        (WRAM_START..=WRAM_END).contains(&address)
    }

    fn read(&self, address: usize) -> u8 {
        let address = address - WRAM_START;
        self.0[address]
    }

    fn write(&mut self, address: usize, value: u8) {
        let address = address - WRAM_START;
        self.0[address] = value;
    }
}

#[derive(Default, Debug)]
struct IoMem {}

impl IoMem {
    fn matches(address: usize) -> bool {
        (IO_START..=IO_END).contains(&address)
    }

    fn is_unmapped(address: usize) -> bool {
        (0xFF4C..=0xFF4F).contains(&address) ||
        (0xFF5A..=0xFF5F).contains(&address) ||
        (0xFF78..=0xFF7F).contains(&address) ||
        (0xFF72..=0xFF75).contains(&address) ||
        (0xFF56..=0xFF57).contains(&address) ||
        // GCB only VRAM DMA
        (0xFF51..=0xFF55).contains(&address) ||
        (0xFF60..=0xFF6F).contains(&address) ||
        // GCB only
        (0xFF58..=0xFF59).contains(&address) ||
        // those are only for GCB, we dont do it
        [0xFF77, 0xFF76, 0xFF71, 0xFF70].contains(&address)
    }

    fn read(&self, address: usize) -> Result<u8> {
        anyhow::bail!("Not implemented IO read at: {:#04x}", address)
    }

    fn write(&mut self, address: usize, _value: u8) -> Result<()> {
        if IoMem::is_unmapped(address) {
            // not documented,so NOOP
            tracing::warn!("Write to undocumented IO at: {:#04x}", address);
            return Ok(());
        }

        anyhow::bail!("Not implemented IO write at: {:#04x}", address)
    }
}

#[derive(Debug, Default)]
pub struct Registers {
    pub if_: IfReg,
    ie: IeReg,
    // These two registers specify the top-left coordinates of
    // the visible 160×144 pixel area within the 256×256 pixels BG map. Values in the range 0–255 may be used.
    scx: u8,
    scy: u8,
    sc: SerialControl,
    // These two registers specify the on-screen coordinates of the Window’s top-left pixel.
    wy: u8,
    wx: u8,
    // div: Div,
    /// This timer is incremented at the clock frequency specified by the TAC register.
    /// When the value overflows it is reset to the value specified in TMA and an interrupt is requested, as described below.
    tima: Tima,
    /// When TIMA overflows, it is reset to the value in this register and an interrupt is requested.
    tma: Tma,
    tac: TimerControl,
    // This register assigns gray shades to the color indices of the BG and Window tiles.
    bgp: BgPallet,
    ob_0: ObPallet,
    ob_1: ObPallet,
    lcdc: LcdControl,
    /// LY indicates the current horizontal line, which might be about to be drawn,
    /// being drawn, or just been drawn. LY can hold any value from 0 to 153,
    /// with values from 144 to 153 indicating the VBlank period.
    pub ly: u8,
    /// The eight Game Boy action/direction buttons are arranged as a 2×4 matrix.
    /// Select either action or direction buttons by writing to this register, then read out the bits 0-3.
    p1: Joypad,
    /// Audio Control
    nr52: Nr52,
    nr11: Nr11,
    nr51: Nr51,
    nr50: Nr50,
    pub stat: Stat,
    nr13: Nr13,
    nr14: Nr14,
}

impl Registers {
    fn matches(address: usize) -> bool {
        vec![
            P1_REG, SB_REG, SC_REG, DIV_REG, TIMA_REG, TMA_REG, TAC_REG, IF_REG, NR_10_REG,
            NR_11_REG, NR_12_REG, NR_13_REG, NR_14_REG, NR_21_REG, NR_22_REG, NR_23_REG, NR_24_REG,
            NR_30_REG, NR_31_REG, NR_32_REG, NR_33_REG, NR_34_REG, NR_41_REG, NR_42_REG, NR_43_REG,
            NR_44_REG, NR_50_REG, NR_51_REG, NR_52_REG, LCDC_REG, STAT_REG, SCY_REG, SCX_REG,
            LY_REG, LYC_REG, DMA_REG, BGP_REG, OBP0_REG, OBP1_REG, WY_REG, WX_REG, IE_REG,
        ]
        .contains(&address)
    }

    pub fn get_pending_interrupts(&mut self) -> Vec<InterruptSource> {
        let mut irs: Vec<InterruptSource> = Vec::new();

        // If IME and IE allow the servicing of more than one of the requested interrupts,
        // the interrupt with the highest priority is serviced first.
        // The priorities follow the order of the bits in the IE and IF registers:
        // Bit 0 (VBlank) has the highest priority,
        // and Bit 4 (Joypad) has the lowest priority.

        if self.if_.joypad {
            irs.push(InterruptSource::Joypad);
        }

        if self.if_.serial {
            irs.push(InterruptSource::Serial);
        }

        if self.if_.timer {
            irs.push(InterruptSource::Timer);
        }

        if self.if_.lcd {
            irs.push(InterruptSource::Stat);
        }

        if self.if_.vblank {
            irs.push(InterruptSource::VBlank);
        }
        irs
    }

    pub fn inc_timer(&mut self, n_cycles: u8) {
        if !self.tac.enable {
            return;
        }

        self.tima.pending_cycles += n_cycles as u32;

        if self.tima.pending_cycles < self.tac.clock_select.increment_every {
            return;
        }

        self.tima.value += 1;
        self.tima.pending_cycles -= self.tac.clock_select.increment_every;

        if self.tima.value > 0xFF {
            self.tima.value = self.tma.0 as u32;
            self.if_.timer = true;
        }
    }

    pub fn cycle_duration(&self, n_cycles: u32) -> std::time::Duration {
        let n_cycles = n_cycles as f64;
        let period: f64 = 1f64 / self.tac.clock_select.frequency as f64;
        let duration = (n_cycles * period * 1_000_000f64) as u64;
        std::time::Duration::from_micros(duration)
    }

    // #[tracing::instrument(skip(self), err)]
    fn read(&self, address: usize) -> Result<u8> {
        let value = match address {
            IF_REG => self.if_.get(),
            IE_REG => self.ie.get(),

            LY_REG => self.ly,

            P1_REG => self.p1.get(),

            SCX_REG => self.scx,

            SCY_REG => self.scy,

            _ => {
                anyhow::bail!("Not implemented read: {:#x}", address)
            }
        };

        Ok(value)
    }

    // #[tracing::instrument(skip(self), err)]
    fn write(&mut self, address: usize, value: u8) -> Result<()> {
        match address {
            IF_REG => {
                self.if_.set(value);
            }

            IE_REG => {
                self.ie.set(value);
            }

            SB_REG => {
                tracing::info!("Serial send: {:#x}", value);
            }

            SC_REG => {
                self.sc.set(value);
            }

            WX_REG => {
                self.wx = value;
            }

            WY_REG => {
                self.wy = value;
            }

            TMA_REG => {
                self.tma.0 = value;
            }

            TAC_REG => {
                self.tac.set(value)?;
            }

            BGP_REG => {
                self.bgp.set(value);
            }

            OBP0_REG => {
                self.ob_0.set(value);
            }

            OBP1_REG => {
                self.ob_1.set(value);
            }

            LCDC_REG => {
                self.lcdc.set(value);
            }

            LY_REG => {
                anyhow::bail!("LCD Y coordinate is read only")
            }

            P1_REG => {
                self.p1.set(value);
            }

            NR_52_REG => {
                self.nr52.set(value);
            }

            NR_11_REG => {
                self.nr11.set(value);
            }

            NR_51_REG => {
                self.nr51.set(value);
            }

            NR_50_REG => {
                self.nr50.set(value);
            }

            STAT_REG => {
                self.stat.set(value);
            }

            NR_13_REG => {
                self.nr13.set(value);
            }

            NR_14_REG => {
                self.nr14.set(value);
            }

            SCY_REG => {
                self.scy = value;
            }

            SCX_REG => {
                self.scx = value;
            }

            _ => {
                anyhow::bail!("Not implemented write: {:#x}", address)
            }
        }

        Ok(())
    }
}

/// This timer is incremented at the clock frequency specified by the TAC register ($FF07).
/// When the value overflows (exceeds $FF) it is reset to the value
/// specified in TMA (FF06) and an interrupt is requested, as described below.
#[derive(Debug, Default)]
struct Tima {
    value: u32,
    pending_cycles: u32,
}

/// When TIMA overflows, it is reset to the value in this register and an interrupt is requested.
/// Example of use: if TMA is set to $FF, an interrupt is requested at the clock frequency selected
/// in TAC (because every increment is an overflow). However, if TMA is set to $FE,
/// an interrupt is only requested every two increments, which effectively
/// divides the selected clock by two. Setting TMA to $FD would divide the clock by three, and so on.
#[derive(Debug, Default)]
struct Tma(u8);

#[derive(Default, Debug)]
struct Joypad {
    /// If this bit is 0, then buttons (SsBA) can be read from the lower nibble.
    select_buttons: bool,
    /// If this bit is 0, then directional keys can be read from the lower nibble.
    select_d_pad: bool,
    start_down: bool,
    select_up: bool,
    b_left: bool,
    a_right: bool,
}

impl Joypad {
    fn get(&self) -> u8 {
        (self.a_right as u8)
            + ((self.b_left as u8) << 1)
            + ((self.select_up as u8) << 2)
            + ((self.start_down as u8) << 3)
            + ((self.select_d_pad as u8) << 4)
            + ((self.select_buttons as u8) << 5)
    }

    fn set(&mut self, value: u8) {
        self.a_right = is_nth_bit_set(value, 0);
        self.b_left = is_nth_bit_set(value, 1);
        self.select_up = is_nth_bit_set(value, 2);
        self.start_down = is_nth_bit_set(value, 3);
        self.select_d_pad = is_nth_bit_set(value, 4);
        self.select_buttons = is_nth_bit_set(value, 5);
    }
}

#[derive(Default, Debug)]
struct TimerControl {
    /// Controls whether TIMA is incremented. Note that DIV is always counting, regardless of this bit
    enable: bool,
    clock_select: ClockSource,
}

#[derive(Debug)]
struct ClockSource {
    // cycles
    increment_every: u32,
    // Hz
    frequency: u32,
}

#[derive(Debug, Default)]
struct BgPallet {
    id_0: Color,
    id_1: Color,
    id_2: Color,
    id_3: Color,
}

#[derive(Debug, Default)]
enum Color {
    #[default]
    White,
    LightGray,
    DarkGray,
    Black,
}

impl BgPallet {
    fn set(&mut self, value: u8) {
        self.id_0 = (is_nth_bit_set(value, 1), is_nth_bit_set(value, 0)).into();
        self.id_1 = (is_nth_bit_set(value, 3), is_nth_bit_set(value, 2)).into();
        self.id_2 = (is_nth_bit_set(value, 5), is_nth_bit_set(value, 4)).into();
        self.id_3 = (is_nth_bit_set(value, 7), is_nth_bit_set(value, 6)).into();
    }
}

/// These registers assigns gray shades to the color indexes of the OBJs that use the corresponding palette.
/// They work exactly like BGP, except that the lower two bits are ignored because color index 0 is transparent for OBJs.
#[derive(Debug, Default)]
struct ObPallet {
    id_1: Color,
    id_2: Color,
    id_3: Color,
}

impl ObPallet {
    fn set(&mut self, value: u8) {
        self.id_1 = (is_nth_bit_set(value, 3), is_nth_bit_set(value, 2)).into();
        self.id_2 = (is_nth_bit_set(value, 5), is_nth_bit_set(value, 4)).into();
        self.id_3 = (is_nth_bit_set(value, 7), is_nth_bit_set(value, 6)).into();
    }
}

impl From<(bool, bool)> for Color {
    fn from(value: (bool, bool)) -> Self {
        // Value	Color
        // 0	White
        // 1	Light gray
        // 2	Dark gray
        // 3	Black
        match value {
            (false, false) => Color::White,
            (false, true) => Color::LightGray,
            (true, false) => Color::DarkGray,
            (true, true) => Color::Black,
        }
    }
}

impl Default for ClockSource {
    fn default() -> Self {
        Self {
            increment_every: 256,
            frequency: 4096,
        }
    }
}

impl TimerControl {
    fn set(&mut self, value: u8) -> Result<()> {
        self.enable = is_nth_bit_set(value, 2);

        let clock_select = value & 0b_0000_0011;

        // Clock select: Controls the frequency at which TIMA is incremented, as follows:
        //
        // Clock    Increment every    Frequency (Hz)
        // 00       256 M-cycles       4096
        // 01       4 M-cycles         262144
        // 10       16 M-cycles        65536
        // 11       64 M-cycles        16384

        let (cycle, freq) = match clock_select {
            0 => (256, 4096),
            1 => (4, 262144),
            2 => (16, 65536),
            3 => (64, 16384),
            _ => {
                anyhow::bail!("Unsupported clock select {:#04x}", clock_select)
            }
        };

        self.clock_select = ClockSource {
            increment_every: cycle,
            frequency: freq,
        };

        Ok(())
    }
}

#[derive(Debug, Default)]
struct LcdControl {
    lcd_ppu_enable: bool,
    // Window tile map area: 0 = 9800–9BFF; 1 = 9C00–9FFF
    window_tile_map_area: (u16, u16),
    window_enable: bool,
    // BG & Window tile data area: 0 = 8800–97FF; 1 = 8000–8FFF
    bg_window_data_area: (u16, u16),
    // BG tile map area: 0 = 9800–9BFF; 1 = 9C00–9FFF
    bg_tile_map_area: (u16, u16),
    // OBJ size: 0 = 8×8; 1 = 8×16
    obj_size: ObjSize,
    obj_enable: bool,
    bg_window_priority_enabled: bool,
}

impl LcdControl {
    fn set(&mut self, value: u8) {
        self.bg_window_priority_enabled = is_nth_bit_set(value, 0);
        self.obj_enable = is_nth_bit_set(value, 1);
        self.obj_size = if is_nth_bit_set(value, 2) {
            ObjSize::Size8x16
        } else {
            ObjSize::Size8x8
        };

        self.bg_tile_map_area = if is_nth_bit_set(value, 3) {
            (0x9C00, 0x9FFF)
        } else {
            (0x9800, 0x9BFF)
        };

        self.bg_window_data_area = if is_nth_bit_set(value, 4) {
            (0x8000, 0x8FFF)
        } else {
            (0x8800, 0x97FF)
        };

        self.window_enable = is_nth_bit_set(value, 5);

        self.window_tile_map_area = if is_nth_bit_set(value, 6) {
            (0x9C00, 0x9FFF)
        } else {
            (0x9800, 0x9BFF)
        };

        self.lcd_ppu_enable = is_nth_bit_set(value, 7);
    }
}

#[derive(Debug, Default)]
enum ObjSize {
    #[default]
    Size8x8,
    Size8x16,
}

/// When an interrupt request signal (some internal wire going from the PPU/APU/… to the CPU)
/// changes from low to high, the corresponding bit in the IF register becomes set.
#[derive(Debug, Default)]
pub struct IfReg {
    joypad: bool,
    serial: bool,
    timer: bool,
    lcd: bool,
    pub vblank: bool,
}

/// Controls whether the ? interrupt handler may be called
#[derive(Debug, Default)]
struct IeReg {
    joypad: bool,
    serial: bool,
    timer: bool,
    lcd: bool,
    vblank: bool,
}

impl IeReg {
    fn set(&mut self, value: u8) {
        self.vblank = is_nth_bit_set(value, 0);
        self.lcd = is_nth_bit_set(value, 1);
        self.timer = is_nth_bit_set(value, 2);
        self.serial = is_nth_bit_set(value, 3);
        self.joypad = is_nth_bit_set(value, 4);
    }

    fn get(&self) -> u8 {
        (self.vblank as u8) + ((self.lcd as u8) << 1) + ((self.timer as u8) << 2)
    }
}

impl IfReg {
    fn set(&mut self, value: u8) {
        if is_nth_bit_set(value, 0) {
            tracing::debug!("Requesting VBlank interrupt");
            self.vblank = true;
        }

        if is_nth_bit_set(value, 1) {
            tracing::debug!("Requesting LCD interrupt");
            self.lcd = true;
        }

        if is_nth_bit_set(value, 2) {
            tracing::debug!("Requesting timer interrupt");
            self.timer = true;
        }

        if is_nth_bit_set(value, 3) {
            tracing::debug!("Requesting serial interrupt");
            self.serial = true;
        }

        if is_nth_bit_set(value, 4) {
            tracing::debug!("Requesting joypad interrupt");
            self.joypad = true;
        }
    }

    fn get(&self) -> u8 {
        (self.vblank as u8)
            + ((self.lcd as u8) << 1)
            + ((self.timer as u8) << 2)
            + ((self.serial as u8) << 3)
            + ((self.joypad as u8) << 4)
    }

    pub fn acknowledge(&mut self, interrupt: &InterruptSource) {
        match interrupt {
            InterruptSource::VBlank => self.vblank = false,
            InterruptSource::Stat => self.lcd = false,
            InterruptSource::Timer => self.timer = false,
            InterruptSource::Serial => self.serial = false,
            InterruptSource::Joypad => self.joypad = false,
        }
    }
}

#[derive(Default, Debug)]
struct SerialControl {
    enable: bool,
    // If set to 1, enable high speed serial clock (~256 kHz in single-speed mode)
    high_speed_clock: bool,
    // 0 = External clock (“slave”), 1 = Internal clock (“master”)
    clock_select: bool,
}

impl SerialControl {
    fn set(&mut self, value: u8) {
        self.enable = is_nth_bit_set(value, 0);

        if self.enable {
            tracing::warn!("Serial not implemented");
        }

        self.high_speed_clock = is_nth_bit_set(value, 1);
        self.clock_select = is_nth_bit_set(value, 2);
    }
}

/// NR52: Audio master control
#[derive(Debug, Default)]
struct Nr52 {
    audio_on: bool,
    ch4_on: bool,
    ch3_on: bool,
    ch2_on: bool,
    ch1_on: bool,
}

impl Nr52 {
    fn set(&mut self, value: u8) {
        self.audio_on = is_nth_bit_set(value, 7);
        self.ch4_on = is_nth_bit_set(value, 3);
        self.ch3_on = is_nth_bit_set(value, 2);
        self.ch2_on = is_nth_bit_set(value, 1);
        self.ch1_on = is_nth_bit_set(value, 0);
    }
}

///  Channel 1 length timer & duty cycle
/// https://gbdev.io/pandocs/Audio_Registers.html#ff11--nr11-channel-1-length-timer--duty-cycle
#[derive(Debug, Default)]
struct Nr11 {}

impl Nr11 {
    fn set(&mut self, _value: u8) {
        tracing::warn!("Nr11 not implemeted")
    }
}

/// https://gbdev.io/pandocs/Audio_Registers.html#ff25--nr51-sound-panning
#[derive(Debug, Default)]
struct Nr51 {}

impl Nr51 {
    fn set(&mut self, _value: u8) {
        tracing::warn!("Nr51 not implemeted")
    }
}

/// https://gbdev.io/pandocs/Audio_Registers.html#ff25--nr51-sound-panning
#[derive(Debug, Default)]
struct Nr50 {}

impl Nr50 {
    fn set(&mut self, _value: u8) {
        tracing::warn!("Nr50 not implemeted")
    }
}

/// Bit 7 - Unused (always 1 on real hardware, some docs mark as 0)
/// Bit 6 - LYC=LY Coincidence Interrupt Enable
/// Bit 5 - Mode 2 (OAM) Interrupt Enable
/// Bit 4 - Mode 1 (V-Blank) Interrupt Enable
/// Bit 3 - Mode 0 (H-Blank) Interrupt Enable
/// Bit 2 - LYC=LY Flag (0=not equal, 1=equal)
/// Bit 1-0 - Mode Flag:
///           00: H-Blank
///           01: V-Blank
///           10: OAM Search
///           11: LCD Transfer

#[derive(Debug, Default)]
pub struct Stat {
    coincidence_ir_enable: bool,
    oam_ir_enable: bool,
    v_blank_ir_enable: bool,
    h_blank_ir_enable: bool,
    lyc_ly: bool,
    pub mode: StatMode,
}

impl Stat {
    /// the LCD STAT register at 0xFF41 is a mix of read-only status bits and writable control bits.
    /// Bit 6 - LYC=LY Coincidence Interrupt Enable   (R/W)
    /// Bit 5 - Mode 2 (OAM) Interrupt Enable         (R/W)
    /// Bit 4 - Mode 1 (V-Blank) Interrupt Enable     (R/W)
    /// Bit 3 - Mode 0 (H-Blank) Interrupt Enable     (R/W)
    /// Bit 2 - LYC=LY Flag                           (Read-only)
    /// Bit 1-0 - Mode Flag (current PPU mode)        (Read-only)
    fn set(&mut self, value: u8) {
        self.coincidence_ir_enable = is_nth_bit_set(value, 6);
        self.oam_ir_enable = is_nth_bit_set(value, 5);
        self.v_blank_ir_enable = is_nth_bit_set(value, 4);
        self.h_blank_ir_enable = is_nth_bit_set(value, 3);
    }
}

/// Important notes
/// The STAT register itself is read-only for the mode bits — the PPU sets them automatically depending on the current scanline timing.
/// You can only control the interrupt enable bits (STAT[3–6]).
/// Emulators typically initialize LY = 0 and mode = 2 at boot.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum StatMode {
    Hblank,
    Vblank,
    #[default]
    OamSearch,
    LcdTransfer,
}

/// Channel 1 Frequency Low
/// Channel 1 is the Square wave with sweep channel.
/// The frequency of this channel is 11 bits wide, split across two registers:
/// NR13 (0xFF13) → low 8 bits of frequency (freq[7:0])

#[derive(Debug, Default)]
struct Nr13 {}

impl Nr13 {
    fn set(&mut self, _value: u8) {
        tracing::warn!("Nr13 not implemented")
    }
}

/// NR14 (0xFF14) → high 3 bits of frequency (freq[10:8]) + control flags

#[derive(Debug, Default)]
struct Nr14 {}

impl Nr14 {
    fn set(&mut self, _value: u8) {
        tracing::warn!("Nr14 not implemented")
    }
}
