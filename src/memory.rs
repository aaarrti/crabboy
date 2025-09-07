use crate::cartridge::Cartridge;
use crate::util::{is_nth_bit_set, set_nth_bit};
use derivative::Derivative;
use std::fmt::Debug;
use std::path::PathBuf;
use anyhow::{anyhow, Result};
use crate::{DISPLAY_HEIGHT, DISPLAY_WIDTH};

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
    //pub cartridge: Cartridge,
    boot_rom_mapped: bool,
    io_: IoMem,
    oam: ObjectAttributeMemory,
    rom_size: usize
}

impl Memory {

    fn from_test_rom(path: &PathBuf) -> Self {
        let data: Vec<u8> = std::fs::read(path).unwrap();

        let mut rom_0 = [0; ROM_0_END + 1];
        let vram = Vram::new();
        let wram = Wram::new();
        let io_ = IoMem::default();

        // mmap first 16KB
        rom_0.copy_from_slice(&data[0..=ROM_0_END]);
        // rom_n.copy_from_slice(&cartridge[ROM_N_START..ROM_N_END + 1]);

        let hram = Hram::new();
        let registers = Registers::default();


        let boot_rom_ = [0; BOOT_ROM_END + 1];

        Memory {
            boot_rom: boot_rom_,
            rom_0,
            vram,
            wram,
            hram,
            registers,
            rom_size: data.len(),
            io_,
            boot_rom_mapped: false,
            oam: ObjectAttributeMemory::default(),
        }
    }

    fn from_cartridge(path: &PathBuf) -> Self {
        let boot_rom: Vec<u8> = std::fs::read("data/boot.gb").unwrap();
        let cartridge = Cartridge::new(path);

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

        Memory {
            boot_rom: boot_rom_,
            rom_0,
            vram,
            wram,
            hram,
            registers,
            //cartridge,
            io_,
            boot_rom_mapped: true,
            oam: ObjectAttributeMemory::default(),
            rom_size: cartridge.header.rom_size
        }

    }


    pub fn new(path: &PathBuf, test_rom: bool) -> Self {
        if test_rom {
            Memory::from_test_rom(path)
        } else {
            Memory::from_cartridge(path)
        }
    }

    pub fn read(&self, address: u16) -> u8 {
        let address = address as usize;

        if self.boot_rom_mapped && address <= BOOT_ROM_END {
            return self.boot_rom[address];
        }
        if Registers::matches(address) {
            // tracing::debug!("Read from hardware register at: {:#04x}", address);
            return self.registers.read(address);
        }

        if address <= ROM_0_END {
            let limit = self.rom_size;
            if address >= limit {
                panic!(
                    "Address {:#x} out of bounds for ROM size {:#x}",
                    address, limit
                );
            }
            return self.rom_0[address];
        }

        if (ROM_N_START..=ROM_N_END).contains(&address) {
            // let address = address - ROM_N_START;
            // return Ok(self.rom_n[address]);
            panic!("ROM n not implemeted")
        }

        if Vram::matches(address) {
            return self.vram.read(address);
        };

        if (EX_RAM_START..=EX_RAM_END).contains(&address) {
            panic!("EX RAM no implemeted");
        }

        if Wram::matches(address) {
            return self.wram.read(address);
        }

        if (ECHO_RAM_START..=ECHO_RAM_END).contains(&address) {
            panic!("Illegal read to ECHO RAM at {:#04x}", address)
        }

        if (OAM_START..=OAM_END).contains(&address) {
            panic!("OAM read not implemted")
        }

        if (NOT_USABLE_START..=NOT_USABLE_END).contains(&address) {
            panic!("Illegal read to NOT USABLE at {:#04x}", address)
        }

        if IoMem::matches(address) {
            return self.io_.read(address);
        }

        if Hram::matches(address) {
            return self.hram.read(address);
        }

        panic!("Illegal address = {:#04x}", address);
    }

    //#[tracing::instrument(skip(self), err)]
    pub fn write(&mut self, address: u16, value: u8) {
        let address = address as usize;

        if address == BOOT_ROM_DISABLE_REG {
            // if self.boot_rom_mapped {
            if self.boot_rom_mapped && value != 0x0 {
                tracing::info!("Boot ROM done executing!");
                self.boot_rom_mapped = false;
                return;
            }
        }
        if address <= BOOT_ROM_END {
            panic!("Illegal write to boot ROM at {:#04x}", address);
        }
        // }
        if Registers::matches(address) {
            // tracing::debug!("Write hardware register at: {:#04x}", address);
            return self.registers.write(address, value);
        }

        if address == BANK_SELECT_REGISTER {
            // thos writes are handled by MBC on cartridge
            // rom only cartridge - no bank switch, just ignore
            return;
        }

        if address == IE_REG {
            panic!("IE REG write not implemented");
        }

        if address <= ROM_0_END {
            panic!("Illegal rite to ROM bank 0 at {:#04x}", address)
        }

        if (ROM_N_START..=ROM_N_END).contains(&address) {
            panic!("Illegal write to ROM bank n at {:#04x}", address)
        }

        if Vram::matches(address) {
            self.vram.write(address, value);
            return;
        }

        if (EX_RAM_START..=EX_RAM_END).contains(&address) {
            // let address = address - WRAM_START;
            // self.ex_ram[address] = value;
            // return Ok(());
            panic!("EX RAM no implemeted");
        }

        if Wram::matches(address) {
            self.wram.write(address, value);
            return;
        }

        if (ECHO_RAM_START..=ECHO_RAM_END).contains(&address) {
            panic!("Illegal access to ECHO RAM at {:#04x}", address)
        }

        if (OAM_START..=OAM_END).contains(&address) {
            // CPU access is blocked during Mode 2 & 3 (OAM search and drawing), but allowed in HBlank & VBlank.

            return match &self.registers.stat.mode {
                StatMode::Hblank | StatMode::Vblank => self.oam.write(address, value),

                mode => {
                    tracing::warn!("Blocking OAM write during mode = {:?}", mode);
                }
            };
        }

        if (NOT_USABLE_START..=NOT_USABLE_END).contains(&address) {
            //tracing::warn!("Illegal write to NOT USABLE at {:#04x}", address);
            return;
        }

        if IoMem::matches(address) {
            return self.io_.write(address, value);
        }

        if Hram::matches(address) {
            self.hram.write(address, value);
            return;
        }

        panic!("Illegal address = {:#04x}", address)
    }

    fn tile_data_base_and_index(&self, tile_index: u8) -> u16 {
        let base: u16;
        let offset: u16;

        if self.registers.lcdc.bg_window_data_area {
            base = 0x0000;
            offset = tile_index as u16 * 16u16;
        } else {
            base = 0x1000;
            offset = (sign_extend_i8(tile_index) as u16) * 16u16;
        }
        base + offset
    }

    fn bg_map_base(&self) -> u16 {
        if self.registers.lcdc.bg_tile_map_area {
            0x1C00
        } else {
            0x1800
        }
    }

    fn win_map_base(&self) -> u16 {
        if self.registers.lcdc.window_enable {
            0x1C00
        } else {
            0x1800
        }
    }

    fn fetch_tile_pixel(&self, tile_addr: usize, row_in_tile: usize, col_in_tile: usize) -> u8 {
        // row_in_tile, col_in_tile in 0..7
        let lo = self.vram.0[tile_addr + row_in_tile * 2];
        let hi = self.vram.0[tile_addr + row_in_tile * 2 + 1];
        let bit = 7 - col_in_tile;
        let b0 = (lo >> bit) & 1;
        let b1 = (hi >> bit) & 1;
        // # 0..3 (color index)
        (b1 << 1) | b0
    }

    fn sprite_height(&self) -> u8 {
        match self.registers.lcdc.obj_size {
            ObjSize::Size8x8 => 8,
            ObjSize::Size8x16 => 16,
        }
    }

    /// Inputs (conceptual)
    /// vram[0x2000]            # DMG VRAM window (0x8000–0x9FFF mapped to 0..0x1FFF here)
    /// oam[40]                 # 40 sprites, each {y,x,tile,attr}
    /// LCDC, SCY, SCX, WY, WX  # 0xFF40..0xFF4B
    /// BGP, OBP0, OBP1         # 0xFF47..0xFF49 (DMG palettes)
    /// frame[144][160]         # store DMG shade 0..3 (or expand to RGBA after)
    pub fn decode_framebuffer(&self) -> Vec<u8> {
        const WIDTH: usize = DISPLAY_WIDTH as usize;
        const  HEIGHT: usize = DISPLAY_HEIGHT as usize;
        let mut frame: [[u8; WIDTH]; HEIGHT] = [[0; WIDTH]; HEIGHT];
        let mut frame_meta_coloridx: [[u8; WIDTH]; HEIGHT] = [[0; WIDTH]; HEIGHT];

        for ly in 0..HEIGHT {
            for x in 0..WIDTH {
                let bg_shade: u8;
                let color_idx: u8;

                if !self.registers.lcdc.lcd_ppu_enable
                    || !self.registers.lcdc.bg_window_priority_enabled
                {
                    bg_shade = 0;
                    color_idx = 0;
                } else {
                    let bx = self.registers.scx + x as u8;
                    let by = self.registers.scy + ly as u8;
                    let tile_x = bx >> 3;
                    let tile_y = by >> 3;
                    let row_in_tile = by & 7;
                    let col_in_tile = bx & 7;

                    let map_base = self.bg_map_base();
                    let map_index_addr = map_base + (tile_y as u16) * 32 + tile_x as u16;
                    let tile_index = self.vram.0[map_index_addr as usize];

                    let tile_addr = self.tile_data_base_and_index(tile_index);
                    color_idx = self.fetch_tile_pixel(
                        tile_addr as usize,
                        row_in_tile as usize,
                        col_in_tile as usize,
                    );
                    bg_shade = self.registers.map_palette_dmg(color_idx);
                }

                frame[ly][x] = bg_shade;
                frame_meta_coloridx[ly][x] = color_idx;
            }

            if self.registers.lcdc.window_enable && self.registers.ly >= self.registers.wy {
                for x in 0..159 {
                    if x >= (self.registers.wx - 7) {
                        let wx = x - (self.registers.wx - 7);
                        let wy = self.registers.ly - self.registers.wy;
                        let tile_x = wx >> 3;
                        let tile_y = wy >> 3;

                        let row_in_tile = wy & 7;
                        let col_in_tile = wx & 7;

                        let map_base = self.win_map_base();
                        let map_index_addr = map_base + (tile_y as u16) * 32 + tile_x as u16;
                        let tile_index = self.vram.0[map_index_addr as usize];

                        let tile_addr = self.tile_data_base_and_index(tile_index);
                        let color_idx = self.fetch_tile_pixel(
                            tile_addr as usize,
                            row_in_tile as usize,
                            col_in_tile as usize,
                        );
                        let shade = self.registers.map_palette_dmg(color_idx);

                        frame[ly][x as usize] = shade;
                        frame_meta_coloridx[ly][x as usize] = color_idx; // window replaces BG for priority purposes
                    }
                }
            }

            if self.registers.lcdc.obj_enable {
                let h = self.sprite_height();

                let mut candidates: Vec<u8> = Vec::with_capacity(10);

                for i in 0..39 {
                    let sy_on_screen = self.oam.sprites[i].y.wrapping_sub(16);
                    if ly >= sy_on_screen as usize && ly < (sy_on_screen + h) as usize {
                        candidates.push(i as u8);
                    }
                    if candidates.len() == 10 {
                        break;
                    }
                }
                // For each screen X, overlay sprites (first visible wins)
                for i in candidates {
                    let sx_on_screen = self.oam.sprites[i as usize].x.wrapping_sub(8);
                    if sx_on_screen >= WIDTH as u8 {
                        continue;
                    }
                    //let attr = self.oam.sprites[i as usize].attr;
                    //let tile = self.oam.sprites[i as usize].tile;
                    // 8x16: tile index points to pair; select top/bottom half
                }

                //
                //     if h == 16:
                //         # ignore tile bit0; top uses &~1, bottom uses |1
                //         if (LY - sy_on_screen) < 8:
                //             tile = tile & 0xFE
                //             row_in_tile = (LY - sy_on_screen)
                //         else:
                //             tile = tile | 0x01
                //             row_in_tile = (LY - sy_on_screen) - 8
                //     else:
                //         row_in_tile = (LY - sy_on_screen)
                //
                //     # Y-flip
                //     if attr.bit6 == 1: row_in_tile = 7 - row_in_tile
                //
                //     # Determine VRAM tile address (sprite uses same tiledata mode as BG)
                //     tile_addr = tiledata_base_and_index(tile, LCDC.bit4)
                //
                //     # For each X that sprite covers
                //     for px in 0..7:
                //         x = sx_on_screen + px
                //         if x >= 160: continue
                //
                //         col = px
                //         # X-flip
                //         if attr.bit5 == 1: col = 7 - col
                //
                //         color_idx = fetch_tile_pixel(vram, tile_addr, row_in_tile, col)
                //         if color_idx == 0: continue     # sprite color 0 is transparent
                //
                //         # choose palette
                //         palette = (attr.bit4 == 1) ? OBP1 : OBP0
                //         sprite_shade = map_palette_DMG(color_idx, palette)
                //
                //         # priority: attr bit7 = 1 means "behind BG" (but not behind BG color 0)
                //         bg_idx_here = frame_meta_coloridx_at(LY,x)   # track per-pixel last BG/Win index
                //         if attr.bit7 == 1 and bg_idx_here != 0:
                //             continue  # hidden behind nonzero BG/Win
                //
                //         # draw and do NOT overwrite by later sprites (OAM priority)
                //         frame[LY][x] = sprite_shade
            }
        }

        /*
            for LY in 0..143:
        # 1) Background (unless LCDC.bit0==0; then use color 0 on DMG)
        for x in 0..159:


        # 2) Window overlay (if enabled and covering this LY)
        if LCDC.bit6 == 1 and LY >= WY:


        # 3) Sprites (if enabled)
        if LCDC.bit1 == 1:
            h = sprite_height(LCDC)

            # Collect up to 10 sprites covering this LY, in OAM order
            candidates = []
            for i in 0..39:
                sy_on_screen = oam[i].y.wrapping_sub(16)   # Y position
                if LY >= sy_on_screen and LY < sy_on_screen + h:
                    candidates.push(i)
                    if candidates.len == 10: break


                end for
            end for
            */

        frame.iter().flatten().cloned().collect()
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
    fn write(&mut self, address: usize, value: u8) {
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

            _ => panic!("this is unexpected"),
        }
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

    fn read(&self, address: usize) -> u8 {
        panic!("Not implemented IO read at: {:#04x}", address)
    }

    fn write(&mut self, address: usize, _value: u8) {
        if IoMem::is_unmapped(address) {
            // not documented,so NOOP
            //tracing::warn!("Write to undocumented IO at: {:#04x}", address);
            return;
        }

        panic!("Not implemented IO write at: {:#04x}", address)
    }
}

const VBLANK_IR_BIT: usize = 0;
const LCD_IR_BIT: usize = 1;
const TIMER_IR_BIT: usize = 2;
const SERIAL_IR_BIT: usize = 3;

const JOYPAD_IR_BIT: usize = 4;

#[derive(Derivative, Default)]
#[derivative(Debug)]
pub struct Registers {
    /// pending interrupts
    /// vblank   0
    ///  lcd      1
    ///  timer    2
    ///  serial   3
    ///  joypad   4
    if_: u8,
    /// interrupt enable
    /// vblank   0
    ///  lcd      1
    ///  timer    2
    ///  serial   3
    ///  joypad   4
    ie: u8,
    // These two registers specify the top-left coordinates of
    // the visible 160×144 pixel area within the 256×256 pixels BG map. Values in the range 0–255 may be used.
    scx: u8,
    scy: u8,
    #[derivative(Debug = "ignore")]
    sc: SerialControl,
    // These two registers specify the on-screen coordinates of the Window’s top-left pixel.
    wy: u8,
    wx: u8,
    // div: Div,
    /// This timer is incremented at the clock frequency specified by the TAC register.
    /// When the value overflows it is reset to the value specified in TMA and an interrupt is requested, as described below.
    #[derivative(Debug = "ignore")]
    tima: Tima,
    /// When TIMA overflows, it is reset to the value in this register and an interrupt is requested.
    #[derivative(Debug = "ignore")]
    tma: Tma,
    #[derivative(Debug = "ignore")]
    tac: TimerControl,
    // This register assigns gray shades to the color indices of the BG and Window tiles.
    bgp: u8,
    #[derivative(Debug = "ignore")]
    ob_0: ObPallet,
    #[derivative(Debug = "ignore")]
    ob_1: ObPallet,
    pub lcdc: LcdControl,
    /// LY indicates the current horizontal line, which might be about to be drawn,
    /// being drawn, or just been drawn. LY can hold any value from 0 to 153,
    /// with values from 144 to 153 indicating the VBlank period.
    pub ly: u8,
    /// The eight Game Boy action/direction buttons are arranged as a 2×4 matrix.
    /// Select either action or direction buttons by writing to this register, then read out the bits 0-3.
    #[derivative(Debug = "ignore")]
    p1: Joypad,
    /// Audio Control
    #[derivative(Debug = "ignore")]
    nr52: Nr52,
    #[derivative(Debug = "ignore")]
    nr11: Nr11,
    #[derivative(Debug = "ignore")]
    nr51: Nr51,
    #[derivative(Debug = "ignore")]
    nr50: Nr50,
    pub stat: Stat,
    #[derivative(Debug = "ignore")]
    nr13: Nr13,
    #[derivative(Debug = "ignore")]
    nr14: Nr14,
    lyc: u8,
}

impl Registers {
    fn matches(address: usize) -> bool {
        (P1_REG..=NR_52_REG).contains(&address)
            || (LCDC_REG..=WX_REG).contains(&address)
            || address == IE_REG

        //[
        //    P1_REG, SB_REG, SC_REG, DIV_REG, TIMA_REG, TMA_REG, TAC_REG, IF_REG, NR_10_REG,
        //    NR_11_REG, NR_12_REG, NR_13_REG, NR_14_REG, NR_21_REG, NR_22_REG, NR_23_REG, NR_24_REG,
        //    NR_30_REG, NR_31_REG, NR_32_REG, NR_33_REG, NR_34_REG, NR_41_REG, NR_42_REG, NR_43_REG,
        //    NR_44_REG, NR_50_REG, NR_51_REG, NR_52_REG, LCDC_REG, STAT_REG, SCY_REG, SCX_REG,
        //    LY_REG, LYC_REG, DMA_REG, BGP_REG, OBP0_REG, OBP1_REG, WY_REG, WX_REG, IE_REG,
        // ]
        // .contains(&address)
    }

    #[inline(always)]
    pub fn get_pending_interrupt(&self) -> Option<InterruptSource> {
        // If IME and IE allow the servicing of more than one of the requested interrupts,
        // the interrupt with the highest priority is serviced first.
        // The priorities follow the order of the bits in the IE and IF registers:
        // Bit 0 (VBlank) has the highest priority,
        // and Bit 4 (Joypad) has the lowest priority.

        let pending = self.if_ & self.ie;
        if pending == 0 {
            return None;
        }
        // isolate lowest set bit (VBlank first): x & -x
        let lsb = pending & pending.wrapping_neg();

        // map bit mask -> enum (fast small match; compiles to a few instrs)
        let src = match lsb {
            0x01 => InterruptSource::VBlank, // bit 0
            0x02 => InterruptSource::Stat,   // bit 1
            0x04 => InterruptSource::Timer,  // bit 2
            0x08 => InterruptSource::Serial, // bit 3
            0x10 => InterruptSource::Joypad, // bit 4
            _ => return None,                // unknown/unused bits
        };
        Some(src)
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
            self.request_interrupt(&InterruptSource::Timer);
        }
    }

    pub fn cycle_duration(&self, n_cycles: u8) -> std::time::Duration {
        let n_cycles = n_cycles as f64;
        let period: f64 = 1f64 / self.tac.clock_select.frequency as f64;
        let duration = (n_cycles * period * 1_000_000f64) as u64;
        std::time::Duration::from_micros(duration)
    }

    fn read(&self, address: usize) -> u8 {
        let value = match address {
            IF_REG => self.if_,
            IE_REG => self.ie,

            LY_REG => self.ly,

            P1_REG => self.p1.get(),

            SCX_REG => self.scx,

            SCY_REG => self.scy,

            _ => {
                panic!("Not implemented read: {:#x}", address)
            }
        };

        value
    }

    fn write(&mut self, address: usize, value: u8) {
        match address {
            IF_REG => {
                self.if_ = value;
            }

            IE_REG => {
                self.ie = value;
            }

            SB_REG => {
                tracing::debug!("Serial send: {:#x}", value);
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
                self.tac.set(value);
            }

            BGP_REG => {
                self.bgp = value;
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
                panic!("LCD Y coordinate is read only")
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

            LYC_REG => {
                self.lyc = value;
            }

            _ => {
                panic!("Not implemented write: {:#x}", address)
            }
        }
    }

    pub fn request_interrupt(&mut self, interrupt_source: &InterruptSource) {
        match interrupt_source {
            InterruptSource::VBlank => {
                if is_nth_bit_set(self.ie, VBLANK_IR_BIT) {
                    self.if_ = set_nth_bit(self.if_, VBLANK_IR_BIT);
                }
            }
            InterruptSource::Stat => {
                if is_nth_bit_set(self.ie, LCD_IR_BIT) {
                    self.if_ = set_nth_bit(self.if_, LCD_IR_BIT);
                }
            }
            InterruptSource::Timer => {
                if is_nth_bit_set(self.ie, TIMER_IR_BIT) {
                    self.if_ = set_nth_bit(self.if_, TIMER_IR_BIT);
                }
            }
            InterruptSource::Serial => {
                if is_nth_bit_set(self.ie, SERIAL_IR_BIT) {
                    self.if_ = set_nth_bit(self.if_, SERIAL_IR_BIT);
                }
            }
            InterruptSource::Joypad => {
                if is_nth_bit_set(self.ie, JOYPAD_IR_BIT) {
                    self.if_ = set_nth_bit(self.if_, JOYPAD_IR_BIT);
                }
            }
        }
    }

    pub fn update_stat_coincidence(&mut self) {
        let coinc_now = self.ly == self.lyc;
        self.stat.lyc_ly = coinc_now;
        if coinc_now && self.stat.coincidence_ir_enable {
            self.if_ = set_nth_bit(self.if_, LCD_IR_BIT);
        }
    }

    pub fn acknowledge_interrupt(&mut self, interrupt: &InterruptSource) {
        let bit_idx = match interrupt {
            InterruptSource::VBlank => VBLANK_IR_BIT,
            InterruptSource::Stat => LCD_IR_BIT,
            InterruptSource::Timer => TIMER_IR_BIT,
            InterruptSource::Serial => SERIAL_IR_BIT,
            InterruptSource::Joypad => JOYPAD_IR_BIT,
        };
        self.if_ = set_nth_bit(self.if_, bit_idx);
    }

    fn map_palette_dmg(&self, idx: u8) -> u8 {
        (self.bgp >> (idx * 2)) & 0b11
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
enum Color {
    #[default]
    White,
    LightGray,
    DarkGray,
    Black,
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
    fn set(&mut self, value: u8) {
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
                panic!("Unsupported clock select {:#04x}", clock_select)
            }
        };

        self.clock_select = ClockSource {
            increment_every: cycle,
            frequency: freq,
        };
    }
}

#[derive(Debug, Default)]
pub struct LcdControl {
    /// bit 7
    pub lcd_ppu_enable: bool,
    // Window tile map area: 0 = 9800–9BFF; 1 = 9C00–9FFF
    /// bit 6
    window_tile_map_area: (u16, u16),
    /// bit 5
    window_enable: bool,
    // BG & Window tile data area: 0 = 8800–97FF; 1 = 8000–8FFF
    /// bit 4
    bg_window_data_area: bool,
    // BG tile map area: 0 = 9800–9BFF; 1 = 9C00–9FFF
    /// bit 3
    bg_tile_map_area: bool,
    // OBJ size: 0 = 8×8; 1 = 8×16
    /// bit 2
    obj_size: ObjSize,
    /// bit 1
    /// sprites enabled
    obj_enable: bool,
    /// bit 0
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

        self.bg_tile_map_area = is_nth_bit_set(value, 3);
        //self.bg_tile_map_area = if is_nth_bit_set(value, 3) {
        //    (0x9C00, 0x9FFF)
        //} else {
        //    (0x9800, 0x9BFF)
        //};

        self.bg_window_data_area = is_nth_bit_set(value, 4);
        //self.bg_window_data_area = if is_nth_bit_set(value, 4) {
        //    (0x8000, 0x8FFF)
        //} else {
        //    (0x8800, 0x97FF)
        //};

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

/// This register selects which of the 4 sound channels (1–4) go to left and/or right outputs.
#[derive(Debug, Default)]
struct Nr51 {
    ch4_right: bool,
    ch3_right: bool,
    ch2_right: bool,
    ch1_right: bool,
    ch4_left: bool,
    ch3_left: bool,
    ch2_left: bool,
    ch1_left: bool,
}

impl Nr51 {
    fn set(&mut self, value: u8) {
        self.ch4_right = is_nth_bit_set(value, 7);
        self.ch3_right = is_nth_bit_set(value, 6);
        self.ch2_right = is_nth_bit_set(value, 5);
        self.ch1_right = is_nth_bit_set(value, 4);
        self.ch4_left = is_nth_bit_set(value, 3);
        self.ch3_left = is_nth_bit_set(value, 2);
        self.ch2_left = is_nth_bit_set(value, 1);
        self.ch1_left = is_nth_bit_set(value, 0);
    }
}

#[derive(Debug, Default)]
struct OutputLevel(u8);

impl From<u8> for OutputLevel {
    fn from(value: u8) -> Self {
        if value <= 7 {
            OutputLevel(value)
        } else {
            panic!("Output level must be in range [0, 7]")
        }
    }
}

/// Bit 7-4 – SO2 (Right speaker) output level (0–7)
/// Bit 3   – Vin to SO2 (1 = enable, mixes external Vin into right output)
/// Bit 2-0 – SO1 (Left speaker) output level (0–7)
/// Bit 0   – Vin to SO1 (1 = enable, mixes external Vin into left output)
#[derive(Debug, Default)]
struct Nr50 {
    right_speaker: OutputLevel,
    right_vin: bool,
    left_speaker: OutputLevel,
    left_vin: bool,
}

impl Nr50 {
    fn set(&mut self, value: u8) {
        let right_speaker: u8 = (value >> 4) & 0x0F;
        self.right_speaker = right_speaker.into();
        self.right_vin = is_nth_bit_set(value, 3);
        let left_speaker: u8 = value & 0x07; // 0x07 = 0000_0111
        self.left_speaker = left_speaker.into();
        self.left_vin = is_nth_bit_set(value, 0);
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
    pub coincidence_ir_enable: bool,
    pub oam_ir_enable: bool,
    pub v_blank_ir_enable: bool,
    pub h_blank_ir_enable: bool,
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
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
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

#[inline(always)]
fn sign_extend_i8(x: u8) -> i16 {
    (x as i8) as i16
}
