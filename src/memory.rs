use crate::cartridge::Cartridge;
use crate::util::{is_nth_bit_set, set_nth_bit};
use crate::{DISPLAY_HEIGHT, DISPLAY_WIDTH};
use derivative::Derivative;
use std::fmt::Debug;
use std::path::PathBuf;

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

//const NR_12_REG: usize = 0xFF12;
const NR_13_REG: usize = 0xFF13;
const NR_14_REG: usize = 0xFF14;
// const NR_21_REG: usize = 0xFF16;
//const NR_22_REG: usize = 0xFF17;
//const NR_23_REG: usize = 0xFF18;
//const NR_24_REG: usize = 0xFF19;
const NR_30_REG: usize = 0xFF1A;

//const NR_31_REG: usize = 0xFF1B;
//const NR_32_REG: usize = 0xFF1C;
//const NR_33_REG: usize = 0xFF1D;
//const NR_34_REG: usize = 0xFF1E;
const NR_41_REG: usize = 0xFF20;
//const NR_42_REG: usize = 0xFF21;
//const NR_43_REG: usize = 0xFF22;
//const NR_44_REG: usize = 0xFF23;
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

//const TILE_DATA_START: usize = 0x8000;
//const TILE_DATA_END: usize = 0x97FF;
//const BG_MAP_0_START: usize = 0x9800;
//const BG_MAP_0_END: usize = 0x9BFF;
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
    pub fn jump_address(&self) -> u16 {
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
    rom_0: [u8; ROM_0_END + 1],
    vram: Vram,
    pub registers: Registers,
    // 2 banks together
    wram: Wram,
    hram: Hram,
    io_: IoMem,
    oam: [u8; OAM_END - OAM_START + 1],
    _rom_size: usize,
}

impl Memory {
    pub fn new(path: &PathBuf) -> Self {
        let cartridge = Cartridge::new(path);

        let mut rom_0 = [0; ROM_0_END + 1];
        let vram = Vram::new();
        let wram = Wram::new();
        let io_ = IoMem::default();

        // mmap first 16KB
        let data = cartridge.data.as_slice();
        rom_0.copy_from_slice(&data[0..=ROM_0_END]);

        let hram = Hram::new();
        let registers = Registers::default();

        let oam = [0; OAM_END - OAM_START + 1];

        let mut memory = Memory {
            rom_0,
            vram,
            wram,
            hram,
            registers,
            //cartridge,
            io_,
            oam,
            _rom_size: cartridge.header.rom_size,
        };
        tracing::info!("Initializing memory");
        // Use these DMG/MGB post-boot values recorded at PC=$0100:
        // FF00 P1/JOYP = 0xCF
        // FF01 SB       = 0x00
        // FF02 SC       = 0x7E
        // FF04 DIV      = 0xAB   ; timing-dependent; see note below
        // FF05 TIMA     = 0x00
        // FF06 TMA      = 0x00
        // FF07 TAC      = 0xF8
        // FF0F IF       = 0xE1
        // ; APU
        // FF10 NR10 = 0x80   FF11 NR11 = 0xBF   FF12 NR12 = 0xF3
        // FF13 NR13 = 0xFF   FF14 NR14 = 0xBF
        // FF16 NR21 = 0x3F   FF17 NR22 = 0x00   FF18 NR23 = 0xFF   FF19 NR24 = 0xBF
        // FF1A NR30 = 0x7F   FF1B NR31 = 0xFF   FF1C NR32 = 0x9F   FF1D NR33 = 0xFF   FF1E NR34 = 0xBF
        // FF20 NR41 = 0xFF   FF21 NR42 = 0x00   FF22 NR43 = 0x00   FF23 NR44 = 0xBF
        // FF24 NR50 = 0x77   FF25 NR51 = 0xF3   FF26 NR52 = 0xF1
        // ; PPU
        // FF40 LCDC = 0x91   FF41 STAT = 0x85
        // FF42 SCY  = 0x00   FF43 SCX  = 0x00
        // FF44 LY   = 0x00   FF45 LYC  = 0x00
        // FF46 DMA  = 0xFF
        // FF47 BGP  = 0xFC
        // FF48 OBP0 = (undefined)   FF49 OBP1 = (undefined)  ; usually 00 or FF on power-up
        // FF4A WY   = 0x00   FF4B WX   = 0x00
        // FFFF IE   = 0x00

        memory.write(P1_REG as u16, 0xCF);
        memory.write(SB_REG as u16, 0x00);
        memory.write(SC_REG as u16, 0x00);
        memory.write(DIV_REG as u16, 0xAB);
        memory.write(TIMA_REG as u16, 0x00);
        memory.write(TMA_REG as u16, 0x00);
        memory.write(TAC_REG as u16, 0xF8);
        memory.write(IF_REG as u16, 0xE1);
        // APU
        memory.write(NR_10_REG as u16, 0x80);
        memory.write(NR_13_REG as u16, 0xFF);
        memory.write(NR_30_REG as u16, 0x7F);
        memory.write(NR_41_REG as u16, 0xFF);
        memory.write(NR_50_REG as u16, 0x77);
        //
        memory.write(LCDC_REG as u16, 0x91);
        memory.write(SCY_REG as u16, 0x00);

        //memory.write(LY_REG as u16, 0x00);
        memory.write(DMA_REG as u16, 0xFF);
        memory.write(BGP_REG as u16, 0xFC);
        memory.write(OBP0_REG as u16, 0x00);
        memory.write(OBP1_REG as u16, 0x00);
        memory.write(IE_REG as u16, 0x00);

        tracing::info!("Memory initialized");
        memory
    }

    pub fn read(&self, address: u16) -> u8 {
        let address = address as usize;

        match address {
            0..=ROM_0_END => self.rom_0[address],

            P1_REG..=NR_52_REG | LCDC_REG..=WX_REG | IE_REG => self.registers.read(address),

            ROM_N_START..=ROM_N_END => {
                // let address = address - ROM_N_START;
                // return Ok(self.rom_n[address]);
                panic!("ROM n not implemeted")
            }
            VRAM_START..=VRAM_END => self.vram.read(address),

            WRAM_START..=WRAM_END => self.wram.read(address),

            EX_RAM_START..=EX_RAM_END => {
                panic!("EX RAM no implemeted");
            }

            ECHO_RAM_START..=ECHO_RAM_END => {
                // 0xE74D is in Echo RAM (E000–FDFF).
                // It’s just a mirror of WRAM at the same address minus 0x2000.
                self.wram.read(address - ECHO_RAM_START + WRAM_START)
            }

            OAM_START..=OAM_END => {
                panic!("OAM read not implemted")
            }
            NOT_USABLE_START..=NOT_USABLE_END => {
                panic!("Illegal read to NOT USABLE at {:#04x}", address)
            }

            IO_START..=IO_END => self.io_.read(address),

            HRAM_START..=HRAM_END => self.hram.read(address),

            _ => {
                panic!("Illegal address = {:#04x}", address)
            }
        }
    }

    //#[tracing::instrument(skip(self), err)]
    pub fn write(&mut self, address: u16, value: u8) {
        let address = address as usize;

        match address {
            BOOT_ROM_DISABLE_REG => {}

            BANK_SELECT_REGISTER => {
                // those writes are handled by MBC on cartridge
                // rom only cartridge - no bank switch, just ignore
            }

            P1_REG..=NR_52_REG | LCDC_REG..=WX_REG | IE_REG => {
                self.registers.write(address, value);
            }

            0..=ROM_0_END => {
                panic!("Illegal rite to ROM bank 0 at {:#04x}", address)
            }

            ROM_N_START..=ROM_N_END => {
                panic!("Illegal write to ROM bank n at {:#04x}", address)
            }

            EX_RAM_START..=EX_RAM_END => {
                panic!("EX RAM no implemeted");
            }

            VRAM_START..=VRAM_END => {
                self.vram.write(address, value);
            }
            ECHO_RAM_START..=ECHO_RAM_END => {
                self.wram.0[address - ECHO_RAM_START] = value;
            }

            OAM_START..=OAM_END => {
                match &self.registers.stat.mode {
                    StatMode::Hblank | StatMode::Vblank => {
                        self.oam[address - OAM_START] = value;
                    }

                    mode => {
                        tracing::warn!("Blocking OAM write during mode = {:?}", mode);
                    }
                };
            }

            NOT_USABLE_START..=NOT_USABLE_END => {}

            IO_START..=IO_END => {
                self.io_.write(address, value);
            }

            HRAM_START..=HRAM_END => {
                self.hram.write(address, value);
            }

            WRAM_START..=WRAM_END => {
                self.wram.write(address, value);
            }

            _ => {
                panic!("Illegal address = {:#04x}", address)
            }
        }
    }

    pub fn tick(&mut self, num_cycles: u8) {
        self.registers
            .serial
            .tick(num_cycles, &mut self.registers.if_);
        self.registers
            .timer
            .tick(num_cycles, &mut self.registers.if_);
        self.tick_oam_dma(num_cycles);
    }

    fn tick_oam_dma(&mut self, tcycles: u8) {
        let tcycles = tcycles as u32;
        if !self.registers.oamdma.active {
            return;
        }
        let src_base = (self.registers.oamdma.src_high as u16) << 8;

        let mut t = tcycles + self.registers.oamdma.timer_t;
        while self.registers.oamdma.active && t >= 4 {
            t -= 4;
            let src = src_base.wrapping_add(self.registers.oamdma.idx);
            let dst = OAM_START as u16 + self.registers.oamdma.idx;

            let b = self.read_dma_source(src); // see access rules below
            self.write_oam_dma(dst, b); // write to OAM bypassing normal bus locks
            self.registers.oamdma.idx += 1;
            if self.registers.oamdma.idx == 160 {
                self.registers.oamdma.active = false;
            }
        }
        self.registers.oamdma.timer_t = t;
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

    //fn sprite_height(&self) -> u8 {
    //    match self.registers.lcdc.obj_size {
    //        ObjSize::Size8x8 => 8,
    //        ObjSize::Size8x16 => 16,
    //    }
    //}

    /// Inputs (conceptual)
    /// vram[0x2000]            # DMG VRAM window (0x8000–0x9FFF mapped to 0..0x1FFF here)
    /// oam[40]                 # 40 sprites, each {y,x,tile,attr}
    /// LCDC, SCY, SCX, WY, WX  # 0xFF40..0xFF4B
    /// BGP, OBP0, OBP1         # 0xFF47..0xFF49 (DMG palettes)
    /// frame[144][160]         # store DMG shade 0..3 (or expand to RGBA after)
    pub fn decode_framebuffer(&self) -> Vec<u8> {
        const WIDTH: usize = DISPLAY_WIDTH as usize;
        const HEIGHT: usize = DISPLAY_HEIGHT as usize;
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
                    let by = self.registers.scy.wrapping_add(ly as u8);
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
        }

        frame.iter().flatten().cloned().collect()
    }

    /// Read a source byte for OAM DMA (FF46). This bypasses CPU access locks.
    pub fn read_dma_source(&self, address: u16) -> u8 {
        let address = address as usize;
        match address {
            // 16 KiB fixed ROM bank
            0..=ROM_0_END => self.rom_0[address],

            // 16 KiB switchable ROM bank (use your mapper; placeholder shown)
            ROM_N_START..=ROM_N_END => {
                panic!("ROM X not supported");
            }

            // VRAM: RAW read (no CPU LCD-mode restrictions)
            VRAM_START..=VRAM_END => self.vram.read(address),

            // External (cartridge) RAM if you have it; else 0xFF
            EX_RAM_START..=EX_RAM_END => {
                panic!("EX-RAM not supported");
            }

            // WRAM (C000–DFFF)
            WRAM_START..=WRAM_END => self.wram.read(address),

            // Echo RAM (E000–FDFF) mirrors WRAM (C000–DDFF)
            ECHO_RAM_START..=ECHO_RAM_END => self.wram.read(address - ECHO_RAM_START + WRAM_START),

            // OAM as source: treat as 0xFF
            OAM_START..=OAM_END => 0xFF,

            // Unusable area
            0xFEA0..=0xFEFF => 0xFF,

            // I/O registers: safest to return 0xFF as source
            IO_START..=IO_END => 0xFF,

            // HRAM
            HRAM_START..=HRAM_END => self.hram.read(address),

            // IE register (FFFF) – return 0xFF as source
            IE_REG => 0xFF,

            _ => {
                panic!("Illegal read from OAM DMA at address {:#x}", address);
            }
        }
    }

    /// Write directly into OAM for DMA (FE00–FE9F), bypassing CPU OAM locks.
    pub fn write_oam_dma(&mut self, addr: u16, val: u8) {
        let addr = addr as usize;
        debug_assert!((OAM_START..=OAM_END).contains(&addr));
        let i = addr - OAM_START;
        self.oam[i] = val;
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

#[derive(Debug, Default)]
struct Serial {
    sb: u8, // FF01
    sc: u8, // FF02 (only bits 7 and 0 meaningful; others read as 1)
    active: bool,
    timer: u32, // T-cycles remaining
    last_tx: u8,
    line_buf: String,
}

impl Serial {
    fn read(&self, addr: usize) -> u8 {
        match addr {
            SB_REG => self.sb,
            SC_REG => {
                let mut v = (self.sc & 0x81) | 0x7E;
                if self.active {
                    v |= 0x01;
                } else {
                    v &= !0x01;
                }
                v
            }
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: usize, val: u8, _if_reg: &mut u8) {
        match addr {
            SB_REG => {
                if !self.active {
                    self.sb = val;
                }
                // (writes during active usually have no effect)
            }
            SC_REG => {
                // store only the meaningful bits, make others read as 1
                self.sc = (val & 0x81) | 0x7E;

                let start = (val & 0x01) != 0;
                // let internal = (val & 0x80) != 0;

                if start {
                    //if internal {
                    // one byte @ 8 kHz -> 8 bits -> 4096 T-cycles on DMG
                    self.active = true;
                    self.timer = 4096; // ~1 ms @ DMG
                    self.last_tx = self.sb; // capture TX at start
                                            //} else {
                                            // external clock: with no peer, never progresses
                    self.active = true;
                    self.timer = u32::MAX; // or some sentinel
                                           //}
                }
            }
            _ => {
                panic!("invalid address for serial {:#x}", addr)
            }
        }
    }

    // call this as your CPU advances time (in T-cycles)
    fn tick(&mut self, tcycles: u8, if_reg: &mut u8) {
        let tcycles = tcycles as u32;
        if !self.active {
            return;
        }

        if (self.sc & 0x80) == 0 {
            // external clock: do nothing (no peer)
            return;
        }

        if self.timer > tcycles {
            self.timer -= tcycles;
        } else {
            // complete
            self.active = false;
            self.timer = 0;
            self.sc &= !0x01; // clear start bit
            self.sb = 0xFF; // received byte (no peer)
            *if_reg |= 0x08; // IF.serial

            //if self.log_enabled {
            let ch = self.last_tx as char;
            if ch == '\n' || ch == '\r' {
                eprintln!("[SER] {}", self.line_buf);
                self.line_buf.clear();
            } else if self.last_tx.is_ascii_graphic() || ch == ' ' {
                self.line_buf.push(ch);
            } else {
                use std::fmt::Write;
                let _ = write!(self.line_buf, "\\x{:02X}", self.last_tx);
            }
            // }
        }
    }
}

impl Vram {
    fn new() -> Self {
        Self([0; VRAM_END - VRAM_START + 1])
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

struct Hram([u8; HRAM_END - HRAM_START + 1]);

impl Hram {
    fn new() -> Self {
        Self([0; HRAM_END - HRAM_START + 1])
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
    fn read(&self, address: usize) -> u8 {
        panic!("Not implemented IO read at: {:#04x}", address)
    }

    fn write(&mut self, address: usize, _value: u8) {
        match address {
            // unused IO
            0xFF4C..=0xFF4F | 0xFF5A..=0xFF5F | 0xFF78..=0xFF7F | 0xFF72..=0xFF75 | 0xFF56..=0xFF57
            // GCB only VRAM DMA
            | 0xFF51..=0xFF55 | 0xFF60..=0xFF6F
            // GCB only
            | 0xFF58..=0xFF59
            // those are only for GCB, we dont do it
            | 0xFF77 | 0xFF76 | 0xFF71 | 0xFF70 => {

            }

            _ => {
                panic!("Not implemented IO write at: {:#04x}", address)
            }

        }
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
    // These two registers specify the on-screen coordinates of the Window’s top-left pixel.
    wy: u8,
    wx: u8,
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
    #[derivative(Debug = "ignore")]
    serial: Serial,
    #[derivative(Debug = "ignore")]
    nr10: Nr10,
    #[derivative(Debug = "ignore")]
    nr30: Nr30,
    #[derivative(Debug = "ignore")]
    nr41: Nr41,
    #[derivative(Debug = "ignore")]
    timer: Timer,
    #[derivative(Debug = "ignore")]
    oamdma: OamDma,
}

impl Registers {
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

    fn read(&self, address: usize) -> u8 {
        match address {
            IF_REG => self.if_,
            IE_REG => self.ie,

            LY_REG => self.ly,

            P1_REG => self.p1.read(),

            SCX_REG => self.scx,

            SCY_REG => self.scy,

            SB_REG | SC_REG => self.serial.read(address),

            _ => {
                panic!("Not implemented read: {:#x}", address)
            }
        }
    }

    fn write(&mut self, address: usize, value: u8) {
        match address {
            IF_REG => {
                self.if_ = value;
            }

            IE_REG => {
                self.ie = value;
            }

            WX_REG => {
                self.wx = value;
            }

            WY_REG => {
                self.wy = value;
            }

            TMA_REG => {
                tracing::warn!("TMA reg not implemented");
            }

            TAC_REG => {
                tracing::warn!("TAC not implemented");
            }

            BGP_REG => {
                self.bgp = value;
            }

            OBP0_REG => {
                self.ob_0.write(value);
            }

            OBP1_REG => {
                self.ob_1.write(value);
            }

            LCDC_REG => {
                self.lcdc.write(value);
            }

            LY_REG => {
                panic!("LCD Y coordinate is read only")
            }

            P1_REG => {
                self.p1.write(value);
            }

            NR_52_REG => {
                self.nr52.write(value);
            }

            NR_11_REG => {
                self.nr11.write(value);
            }

            NR_51_REG => {
                self.nr51.write(value);
            }

            NR_50_REG => {
                self.nr50.write(value);
            }

            STAT_REG => {
                self.stat.write(value);
            }

            NR_13_REG => {
                self.nr13.write(value);
            }

            NR_14_REG => {
                self.nr14.write(value);
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

            NR_10_REG => self.nr10.write(value),

            SC_REG | SB_REG => {
                self.serial.write(address, value, &mut self.if_);
            }

            DIV_REG => {
                tracing::warn!("DIV register not implemented");
            }

            TIMA_REG => {
                tracing::warn!("TIMA reg not implemeted");
            }

            NR_30_REG => {
                self.nr30.write(value);
            }

            NR_41_REG => {
                self.nr41.write(value);
            }

            DMA_REG => {
                self.oamdma.write(value);
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
    fn read(&self) -> u8 {
        (self.a_right as u8)
            + ((self.b_left as u8) << 1)
            + ((self.select_up as u8) << 2)
            + ((self.start_down as u8) << 3)
            + ((self.select_d_pad as u8) << 4)
            + ((self.select_buttons as u8) << 5)
    }

    fn write(&mut self, value: u8) {
        self.a_right = is_nth_bit_set(value, 0);
        self.b_left = is_nth_bit_set(value, 1);
        self.select_up = is_nth_bit_set(value, 2);
        self.start_down = is_nth_bit_set(value, 3);
        self.select_d_pad = is_nth_bit_set(value, 4);
        self.select_buttons = is_nth_bit_set(value, 5);
    }
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
    fn write(&mut self, value: u8) {
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
    fn write(&mut self, value: u8) {
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
    fn write(&mut self, value: u8) {
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
    fn write(&mut self, _value: u8) {
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
    fn write(&mut self, value: u8) {
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

/// Bit 7-4 – SO2 (Right speaker) output level (0–7)
/// Bit 3   – Vin to SO2 (1 = enable, mixes external Vin into right output)
/// Bit 2-0 – SO1 (Left speaker) output level (0–7)
/// Bit 0   – Vin to SO1 (1 = enable, mixes external Vin into left output)
#[derive(Debug, Default)]
struct Nr50 {
    right_speaker: u8,
    right_vin: bool,
    left_speaker: u8,
    left_vin: bool,
}

impl Nr50 {
    fn write(&mut self, value: u8) {
        let right_speaker: u8 = (value >> 4) & 0x0F;
        self.right_speaker = right_speaker;
        self.right_vin = is_nth_bit_set(value, 3);
        let left_speaker: u8 = value & 0x07; // 0x07 = 0000_0111
        self.left_speaker = left_speaker;
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
    fn write(&mut self, value: u8) {
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
    fn write(&mut self, _value: u8) {
        tracing::warn!("Nr13 not implemented")
    }
}

/// NR14 (0xFF14) → high 3 bits of frequency (freq[10:8]) + control flags

#[derive(Debug, Default)]
struct Nr14 {}

impl Nr14 {
    fn write(&mut self, _value: u8) {
        tracing::warn!("Nr14 not implemented")
    }
}

fn sign_extend_i8(x: u8) -> i16 {
    (x as i8) as i16
}

#[derive(Debug, Default)]
struct Nr10 {}

impl Nr10 {
    fn write(&mut self, _value: u8) {
        tracing::warn!("Nr10 not implemented")
    }
}

#[derive(Debug, Default)]
struct Nr30 {}

impl Nr30 {
    fn write(&mut self, _value: u8) {
        tracing::warn!("Nr30 not implemented")
    }
}

#[derive(Debug, Default)]
struct Nr41 {}

impl Nr41 {
    fn write(&mut self, _value: u8) {
        tracing::warn!("Nr30 not implemented")
    }
}

/// While OAM DMA is active on DMG, the CPU can only access HRAM ($FF80–$FFFE).
/// Code typically copies a tiny loop into HRAM, writes FF46, then busy-waits until DMA finishes.
/// Also note the PPU can’t read OAM properly during the transfer; most games do OAM DMA in VBlank to avoid sprite glitches
/// Quick checklist
///     Trigger on write to FF46.
///     Copy 160 bytes from (value<<8)|0x00 to $FE00.
///     Duration: 160 M-cycles (640 T-cycles).
///     DMG CPU access while active: HRAM only ($FF80–$FFFE).
///     Read FF46: return last written value.
///     Prefer to run DMA during VBlank to avoid sprite glitches.
#[derive(Debug, Default)]
struct OamDma {
    active: bool,
    src_high: u8, // last written to FF46
    idx: u16,     // 0..=159
    timer_t: u32, // T-cycles until next byte copy
}

impl OamDma {
    fn write(&mut self, val: u8) {
        self.src_high = val;
        self.active = true;
        self.idx = 0;
        self.timer_t = 0; // first byte can be copied immediately after the write completes
    }
}

/// MMIO writes that can cause immediate effects
/// Write DIV:
///     do old = observed(...);
///     div=0;
///     new = observed(...);
///     i f old==1 && new==0, tick TIMA once (with overflow rules).
///     Write TAC: recompute observed before/after; if falling edge, tick TIMA once.
#[derive(Debug, Default)]
struct Timer {
    div: u16, // internal divider (increments every T-cycle)
    tima: u8,
    tma: u8,
    tac: u8,          // bit2: enable, bits1-0: freq
    reload_delay: u8, // 0=none; 1=just overflowed; 2=reload pending (counts down per M-cycle)
}

impl Timer {
    fn bit_for(&self) -> u8 {
        match self.tac & 0b11 {
            0 => 9,
            1 => 3,
            2 => 5,
            _ => 7,
        }
    }

    fn observed(&self) -> u8 {
        if (self.tac & 0x04) == 0 {
            0
        } else {
            ((self.div >> self.bit_for()) & 1) as u8
        }
    }

    fn tick(&mut self, tcycles: u8, if_reg: &mut u8) {
        // Before incrementing div, sample old observed
        let mut old_obs = self.observed();

        // Advance divider by each T; handle falling edges conservatively
        for _ in 0..tcycles {
            self.div = self.div.wrapping_add(1);
            let new_obs = self.observed();
            if old_obs == 1 && new_obs == 0 {
                // TIMA tick
                if self.tima == 0xFF {
                    self.tima = 0x00;
                    self.reload_delay = 2; // over the next 1 M-cycle (4T), then reload
                } else {
                    self.tima = self.tima.wrapping_add(1);
                }
            }
            old_obs = new_obs;

            // Handle the delayed reload timing every M-cycle if you prefer,
            // or just decrement per T and trigger when reaching 0:
            if self.reload_delay > 0 {
                self.reload_delay -= 1;
                if self.reload_delay == 0 {
                    self.tima = self.tma;
                    *if_reg |= 0x04; // request Timer interrupt
                }
            }
        }
    }
}
