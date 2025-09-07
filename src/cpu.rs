use crate::memory::{InterruptSource, Memory};
use crate::util::is_nth_bit_set;
use std::fmt::Debug;

const HIGH_ADDRESS: u16 = 0xFF00;
const GAME_START: u16 = 0x0100;

trait HighAddr {
    fn high_addr(&self) -> u16;
}

impl HighAddr for u8 {
    fn high_addr(&self) -> u16 {
        (*self as u16) + HIGH_ADDRESS
    }
}

#[derive(Default)]
pub struct Cpu {
    registers: Registers,
    flags: Flags,
    /// Interrupt Master Enabled
    ///
    /// IME   Global flag   Affected by DI / EI
    /// IE    0xFFFF        Bitmask: enables each interrupt source Read/write from code
    /// IF    0xFF0F        Bitmask: flags when interrupt occurred Set by hardware or code
    pub ime: bool,
}

impl Debug for Cpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{registers={{ {:?} }}, flags={{ {:?} }}, ime={:?}}}",
            self.registers,
            self.flags,
            if self.ime { 1 } else { 0 }
        )
    }
}

struct Registers {
    /// Accumulator
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
    /// Stack pointer
    sp: u16,
    /// Program Counter
    pc: u16,
}

/// A = 0x01, B = 0x00, C = 0x13, D = 0x00, E = 0xD8, H = 0x01, L = 0x4D.
/// PC = 0x0100, SP = 0xFFFE.
impl Default for Registers {
    fn default() -> Self {
        Registers {
            a: 0x01,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            pc: 0x0100,
            sp: 0xFFFE,
        }
    }
}

impl Registers {
    fn bc(&self) -> u16 {
        join_u16(self.c, self.b)
    }

    fn set_bc(&mut self, value: u16) {
        (self.c, self.b) = split_u16(value);
    }

    fn de(&self) -> u16 {
        join_u16(self.e, self.d)
    }

    fn set_de(&mut self, value: u16) {
        (self.e, self.d) = split_u16(value);
    }

    fn hl(&self) -> u16 {
        join_u16(self.l, self.h)
    }

    fn set_hl(&mut self, value: u16) {
        (self.l, self.h) = split_u16(value);
    }
}

#[derive(Clone)]
struct Flags {
    /// This bit is set if and only if the result of an operation is zero. Used by conditional jumps.
    z: bool,
    /// The Carry Flag (C, or Cy) Is set in these cases:
    ///
    /// - When the result of an 8-bit addition is higher than $FF.
    /// - When the result of a 16-bit addition is higher than $FFFF.
    /// - When the result of a subtraction or comparison is lower than zero (like in Z80 and x86 CPUs, but unlike in 65XX and ARM CPUs).
    /// - When a rotate/shift operation shifts out a “1” bit.
    /// - Used by conditional jumps and instructions such as ADC, SBC, RL, RLA, etc.
    c: bool,
    /// The BCD Flags (N, H). These flags are used by the DAA instruction only.
    ///
    /// - N indicates whether the previous instruction has been a subtraction,
    n: bool,
    /// - H indicates carry for the lower 4 bits of the result.
    ///   DAA also uses the C flag, which must indicate carry for the upper 4 bits.
    ///   After adding/subtracting two BCD numbers, DAA is used to convert the result to BCD format.
    ///   BCD numbers range from $00 to $99 rather than $00 to $FF.
    ///   Because only two flags (C and H) exist to indicate carry-outs of BCD digits,
    ///   DAA is ineffective for 16-bit operations (which have 4 digits), and use for INC/DEC operations (which do not affect C-flag) has limits.
    h: bool,
}

/// F (flags): most emulators use 0xB0 (Z=1, N=0, H=1, C=1). If you want to be exact: H/C are set iff the header checksum ≠ 0x00, otherwise both clear.
impl Default for Flags {
    fn default() -> Self {
        Flags {
            z: true,
            n: false,
            h: true,
            c: true,
        }
    }
}

impl Debug for Flags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Z={:?},C={:?},N={:?},H={:?}",
            if self.z { 1 } else { 0 },
            if self.c { 1 } else { 0 },
            if self.n { 1 } else { 0 },
            if self.h { 1 } else { 0 }
        )
    }
}

impl Debug for Registers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a={:#x},b={:#x},c={:#x},d={:#x},e={:#x},h={:#x},l={:#x},sp={:#06x},pc={:#06x}",
            self.a, self.b, self.c, self.d, self.e, self.h, self.l, self.sp, self.pc
        )
    }
}

impl Cpu {
    #[inline(always)]
    fn fetch_imm8(&mut self, memory: &Memory) -> u8 {
        let byte = memory.read(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        byte
    }

    fn fetch_imm16(&mut self, memory: &Memory) -> u16 {
        let lo = self.fetch_imm8(memory);
        let hi = self.fetch_imm8(memory);
        join_u16(lo, hi)
    }

    /// Push a 16-bit value onto the stack (little-endian)
    fn push_16stk(&mut self, value: u16, memory: &mut Memory) {
        let (lo, hi) = split_u16(value);
        self.registers.sp -= 1;
        memory.write(self.registers.sp, lo); // Push low byte
        self.registers.sp -= 1;
        memory.write(self.registers.sp, hi); // Push high byte
    }

    /// Pop a 16-bit value from the stack (little-endian)
    fn pop_16stk(&mut self, memory: &Memory) -> u16 {
        let hi = memory.read(self.registers.sp);
        self.registers.sp += 1;
        let lo = memory.read(self.registers.sp);
        self.registers.sp += 1;
        join_u16(lo, hi)
    }

    pub fn service_interrupt(&mut self, interrupt: &InterruptSource, memory: &mut Memory) -> u8 {
        // The following interrupt service routine is executed when control is being transferred to an interrupt handler:

        //Two wait states are executed (2 M-cycles pass while nothing happens; presumably the CPU is executing nops during this time).
        //The current value of the PC register is pushed onto the stack, consuming 2 more M-cycles.
        //The PC register is set to the address of the handler (one of: $40, $48, $50, $58, $60). This consumes one last M-cycle.
        //The entire process lasts 5 M-cycles.
        // After each instruction, check if interrupts should fire.
        // If yes, you:
        // Clear IF’s bit for that interrupt,
        // Push PC to the stack,
        // Set PC to the vector address,
        // Clear IME,

        if self.ime {
            tracing::debug!("serving interrupt {:?}", interrupt);
            memory.registers.acknowledge_interrupt(interrupt);
            self.push_16stk(self.registers.pc, memory);
            self.registers.pc = interrupt.jump_address();
            self.ime = false;
        }
        5
    }

    fn register_af(&self) -> u16 {
        let f = (self.flags.z as u8)
            + ((self.flags.n as u8) << 1)
            + ((self.flags.h as u8) << 2)
            + ((self.flags.c as u8) << 3);
        join_u16(self.registers.a, f)
    }

    /// return number of CPU T-cycles the step consumed
    #[tracing::instrument(skip(memory))]
    pub fn step(&mut self, memory: &mut Memory) -> u8 {

        let opcode = self.fetch_imm8(memory);
        match opcode {
            0x00 => {
                tracing::trace!("NOOP");
                4
            }
            0x01 => {
                tracing::trace!("LD BC imm16");
                let imm16 = self.fetch_imm16(memory);
                self.registers.set_bc(imm16);
                12
            }
            0x02 => {
                tracing::trace!("LD (BC) a");
                memory.write(self.registers.bc(), self.registers.a);
                8
            }

            0x04 => {
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC B");
                (self.registers.b, self.flags) = inc(self.registers.b, &self.flags);
                4
            }

            0x03 => {
                tracing::trace!("INC BC");
                self.registers.set_bc(self.registers.bc() + 1);
                8
            }

            0x05 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC B");
                (self.registers.b, self.flags) = dec(self.registers.b, &self.flags);
                4
            }
            0x06 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, n8");
                let imm8 = self.fetch_imm8(memory);
                self.registers.b = imm8;
                8
            }

            0x0B => {
                // [{'name': 'BC', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("DEC BC");
                self.registers.set_bc(self.registers.bc() - 1);
                8
            }
            0x0C => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC C");
                (self.registers.a, self.flags) = inc(self.registers.a, &self.flags);
                4
            }
            0x0D => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC C");
                (self.registers.c, self.flags) = dec(self.registers.c, &self.flags);
                4
            }
            0x0E => {
                // [{'name': 'C', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, n8");
                let imm8 = self.fetch_imm8(memory);
                self.registers.c = imm8;
                8
            }

            0x09 => {
                tracing::trace!("ADD HL");
                self.registers
                    .set_hl(self.registers.hl() + self.registers.bc());
                8
            }

            0x11 => {
                tracing::trace!("LD DE, n16");
                let imm16 = self.fetch_imm16(memory);
                self.registers.set_de(imm16);
                12
            }

            0x13 => {
                tracing::trace!("INC DE");
                self.registers.set_de(self.registers.de() + 1);
                8
            }

            0x15 => {
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC D");
                (self.registers.d, self.flags) = dec(self.registers.d, &self.flags);
                4
            }
            0x16 => {
                tracing::trace!("LD D, n8");
                let imm8 = self.fetch_imm8(memory);
                self.registers.d = imm8;
                8
            }
            0x17 => {
                // {'Z': '0', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLA ");
                (self.registers.a, self.flags) = rla(self.registers.a, &self.flags);
                4
            }
            0x18 => {
                tracing::trace!("JR e8");
                let offset = self.fetch_imm8(memory);
                self.registers.pc = jr(self.registers.pc, offset);
                12
            }

            0x1A => {
                tracing::trace!("LD A, (DE)");
                let a = memory.read(self.registers.de());
                self.registers.a = a;
                8
            }

            0x1C => {
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC E");
                (self.registers.e, self.flags) = inc(self.registers.e, &self.flags);
                4
            }
            0x1D => {
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC E");
                (self.registers.e, self.flags) = dec(self.registers.e, &self.flags);
                4
            }
            0x1E => {
                tracing::trace!("LD E, n8");
                let imm8 = self.fetch_imm8(memory);
                self.registers.e = imm8;
                8
            }
            0x1F => {
                // {'Z': '0', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRA ");
                (self.registers.a, self.flags) = rra(self.registers.a, self.flags.c);
                4
            }
            0x20 => {
                tracing::trace!("JR NZ, e8");
                let offset = self.fetch_imm8(memory);

                if !self.flags.z {
                    self.registers.pc = jr(self.registers.pc, offset);
                    12
                } else {
                    8
                }
            }
            0x21 => {
                tracing::trace!("LD HL, n16");
                let imm16 = self.fetch_imm16(memory);
                self.registers.set_hl(imm16);
                12
            }
            0x22 => {
                tracing::trace!("LD HL, A");
                memory.write(self.registers.hl(), self.registers.a);
                self.registers.set_hl(self.registers.hl() + 1);
                8
            }
            0x23 => {
                tracing::trace!("INC HL");
                self.registers.set_hl(self.registers.hl() + 1);
                8
            }
            0x24 => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC H");
                (self.registers.h, self.flags) = inc(self.registers.h, &self.flags);
                4
            }

            0x28 => {
                tracing::trace!("JR Z, e8");
                let offset = self.fetch_imm8(memory);
                if self.flags.z {
                    self.registers.pc = jr(self.registers.pc, offset);
                    12
                } else {
                    8
                }
            }

            0x2A => {
                tracing::trace!("LD A, (HL)+");
                self.registers.a = memory.read(self.registers.hl());
                self.registers.set_hl(self.registers.hl() + 1);
                8
            }
            0x2B => {
                tracing::trace!("DEC HL");
                self.registers.set_hl(self.registers.hl() - 1);
                8
            }
            0x2C => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC L");
                (self.registers.l, self.flags) = inc(self.registers.l, &self.flags);
                4
            }

            0x2E => {
                tracing::trace!("LD L, n8");
                let imm8 = self.fetch_imm8(memory);
                self.registers.l = imm8;
                8
            }

            0x31 => {
                tracing::trace!("LD SP, n16");
                let imm16 = self.fetch_imm16(memory);
                self.registers.sp = imm16;
                12
            }
            0x32 => {
                tracing::trace!("LD (HL)-, A");
                memory.write(self.registers.hl(), self.registers.a);
                self.registers.set_hl(self.registers.hl() - 1);
                8
            }

            0x36 => {
                tracing::trace!("LD (HL), n8");
                let imm8 = self.fetch_imm8(memory);
                memory.write(self.registers.hl(), imm8);
                12
            }
            0x37 => {
                // {'Z': '-', 'N': '0', 'H': '0', 'C': '1'}
                tracing::trace!("SCF ");
                self.flags.c = true;
                self.flags.n = false;
                self.flags.h = false;
                4
            }

            0x3D => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC A");
                (self.registers.a, self.flags) = dec(self.registers.a, &self.flags);
                4
            }
            0x3E => {
                tracing::trace!("LD A, n8");
                let imm8 = self.fetch_imm8(memory);
                self.registers.a = imm8;
                8
            }

            0x47 => {
                tracing::trace!("LD B, A");
                self.registers.b = self.registers.a;
                4
            }

            0x49 => {
                tracing::trace!("LD C, C");
                4
            }

            0x4F => {
                tracing::trace!("LD C, A");
                self.registers.c = self.registers.a;
                4
            }

            0x57 => {
                tracing::trace!("LD D, A");
                self.registers.d = self.registers.a;
                4
            }

            0x66 => {
                tracing::trace!("LD H, (HL)");
                self.registers.h = memory.read(self.registers.hl());
                8
            }

            0x67 => {
                tracing::trace!("LD H, A");
                self.registers.h = self.registers.a;
                4
            }

            0x73 => {
                tracing::trace!("LD (HL), E");
                memory.write(self.registers.hl(), self.registers.e);
                8
            }

            0x77 => {
                tracing::trace!("LD (HL), A");
                memory.write(self.registers.hl(), self.registers.a);
                8
            }
            0x78 => {
                tracing::trace!("LD A, B");
                self.registers.a = self.registers.b;
                4
            }

            0x7B => {
                tracing::trace!("LD A, E");
                self.registers.a = self.registers.e;
                4
            }
            0x7C => {
                tracing::trace!("LD A, H");
                self.registers.a = self.registers.h;
                4
            }
            0x7D => {
                tracing::trace!("LD A, L");
                self.registers.a = self.registers.l;
                4
            }
            0x7E => {
                tracing::trace!("LD A, (HL)");
                self.registers.a = memory.read(self.registers.hl());
                8
            }

            0x7F => {
                tracing::trace!("LD A, A");
                4
            }

            0x83 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, E");
                (self.registers.a, self.flags) = add(self.registers.a, self.registers.e);
                4
            }

            0x86 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, (HL)");
                let value = memory.read(self.registers.hl());
                (self.registers.a, self.flags) = add(self.registers.a, value);
                8
            }

            0x89 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, C");
                (self.registers.a, self.flags) = adc(self.registers.a, self.registers.c);
                4
            }

            0x90 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, B");
                (self.registers.a, self.flags) = sub(self.registers.a, self.registers.b);
                4
            }

            0x96 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, (HL)");
                let addr = self.registers.hl();
                let value = memory.read(addr);
                self.registers.a -= value;
                8
            }
            0x97 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '1', 'N': '1', 'H': '0', 'C': '0'}
                tracing::trace!("SUB A, A");
                self.registers.a -= self.registers.a;
                self.flags = Flags {
                    z: true,
                    n: true,
                    h: false,
                    c: false,
                };
                4
            }

            0xA1 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, C");
                (self.registers.a, self.flags) = and(self.registers.a, self.registers.c);
                4
            }

            0xA7 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, A");
                (self.registers.a, self.flags) = and(self.registers.a, self.registers.a);
                4
            }

            0xAF => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '1', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, A");
                (self.registers.a, self.flags) = xor(self.registers.a, self.registers.a);
                4
            }

            0xB1 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, C");
                (self.registers.a, self.flags) = or(self.registers.a, self.registers.c);
                4
            }

            0xB5 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, L");
                (self.registers.a, self.flags) = or(self.registers.a, self.registers.l);
                4
            }

            0xBE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, (HL)");
                self.flags = cp(self.registers.a, memory.read(self.registers.hl()));
                8
            }
            0xBF => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '1', 'N': '1', 'H': '0', 'C': '0'}
                tracing::trace!("CP A, A");
                self.flags = cp(self.registers.a, self.registers.a);
                4
            }
            0xC0 => {
                tracing::trace!("RET NZ");
                if !self.flags.z {
                    let addr = self.pop_16stk(memory);
                    self.registers.pc = addr;
                    20
                } else {
                    8
                }
            }
            0xC1 => {
                tracing::trace!("POP BC");
                let value = self.pop_16stk(memory);
                self.registers.set_bc(value);
                12
            }

            0xC3 => {
                tracing::trace!("JP a16");
                let imm16 = self.fetch_imm16(memory);
                self.registers.pc = imm16;
                16
            }

            0xC5 => {
                tracing::trace!("PUSH BC");
                self.push_16stk(self.registers.bc(), memory);
                16
            }

            0xC9 => {
                tracing::trace!("RET ");
                let ret_addr = self.pop_16stk(memory);
                self.registers.pc = ret_addr;
                16
            }
            0xCB => {
                tracing::trace!("PREFIX");
                let prefix_cycles = self.step_prefixed(memory);
                4 + prefix_cycles
            }
            0xCC => {
                tracing::trace!("CALL Z a16");
                let imm16 = self.fetch_imm16(memory);
                if self.flags.z {
                    let return_addr = self.registers.pc + 1;
                    self.push_16stk(return_addr, memory);
                    self.registers.pc = imm16;
                    24
                } else {
                    12
                }
            }
            0xCE => {
                // {"Z": "Z", "N": "0", "H": "H", "C": "C"}
                tracing::trace!("ADC A, n8");
                let imm8 = self.fetch_imm8(memory);
                (self.registers.a, self.flags) = adc(self.registers.a, imm8);
                8
            }
            0xCD => {
                tracing::trace!("CALL a16");
                let imm16 = self.fetch_imm16(memory);
                let return_addr = self.registers.pc + 1;
                self.push_16stk(return_addr, memory);
                self.registers.pc = imm16;
                24
            }

            0xD5 => {
                tracing::trace!("PUSH DE");
                self.push_16stk(self.registers.de(), memory);
                16
            }

            0xDD => {
                // illegal DD
                4
            }

            0xE0 => {
                tracing::trace!("LDH a8, A");
                let imm8 = self.fetch_imm8(memory);
                memory.write(imm8.high_addr(), self.registers.a);
                12
            }

            0xE2 => {
                tracing::trace!("LDH [C], A");
                memory.write(self.registers.c.high_addr(), self.registers.a);
                8
            }

            0xE5 => {
                tracing::trace!("PUSH HL");
                self.push_16stk(self.registers.hl(), memory);
                16
            }

            0xEA => {
                tracing::trace!("LD (a16), A");
                let addr = self.fetch_imm16(memory);
                memory.write(addr, self.registers.a);
                16
            }

            0xF0 => {
                tracing::trace!("LDH A, a8");
                //if self.registers.pc == 0x65 {
                //    tracing::debug!("Waiting for screen frame LY = {:#x}", memory.registers.ly);
                //}
                let imm8 = self.fetch_imm8(memory);
                self.registers.a = memory.read(imm8.high_addr());
                12
            }

            0xF3 => {
                tracing::trace!("DI");
                self.ime = false;
                4
            }

            0xF5 => {
                tracing::trace!("PUSH AF");
                self.push_16stk(self.register_af(), memory);
                16
            }

            0xFB => {
                tracing::trace!("EI ");
                self.ime = true;
                4
            }

            0xFE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, n8");
                let imm8 = self.fetch_imm8(memory);
                self.flags = cp(self.registers.a, imm8);
                8
            }
            _ => panic!("Not implemented opcode={:#x}", opcode),
        }
    }

    fn step_prefixed(&mut self, memory: &mut Memory) -> u8 {
        let opcode = self.fetch_imm8(memory);
        match opcode {
            0x11 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL C");
                (self.registers.a, self.flags) = rl(self.registers.c, &self.flags, true);
                8
            }

            0x7C => {
                // [{'name': '7', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, H");
                self.flags.z = is_nth_bit_set(self.registers.h, 7);
                self.flags.n = false;
                self.flags.h = true;
                8
            }

            _ => panic!("Not implemented prefix_opcode={:#x}", opcode),
        }
    }

    //fn rst(&mut self, target: u8, memory: &mut Memory) -> Result<()> {
    //    self.push_16stk(self.registers.pc, memory)?;
    //    self.registers.pc = target as u16;
    //    Ok(())
    //}
}

/// Split a 16-bit value into low and high bytes (little-endian)
#[inline(always)]
fn split_u16(value: u16) -> (u8, u8) {
    let lo = (value & 0x00FF) as u8;
    let hi = (value >> 8) as u8;
    (lo, hi) // little-endian: low byte first
}

/// Join low and high bytes into a 16-bit value (little-endian)
#[inline(always)]
fn join_u16(lo: u8, hi: u8) -> u16 {
    u16::from_le_bytes([lo, hi])
}

#[inline(always)]
fn adc(a: u8, b: u8) -> (u8, Flags) {
    let (result, carry) = a.overflowing_add(b);
    // Half-carry: if carry from bit 2
    let half_carry = ((a & 0xE) + (b & 0xF)) > 0xF;
    // Set flags
    let flags = Flags {
        z: result == 0,
        n: false,
        h: half_carry,
        c: carry,
    };

    (result, flags)
}

#[inline(always)]
fn dec(a: u8, flags: &Flags) -> (u8, Flags) {
    // { "Z": "Z", "N": "1", "H": "H", "C": "-"}
    let result = a.wrapping_sub(1);

    let flags = Flags {
        h: a & 0x0F == 0,
        z: result == 0,
        n: true,
        c: flags.c,
    };

    (result, flags)
}

#[inline(always)]
fn inc(a: u8, flags: &Flags) -> (u8, Flags) {
    let result = a.wrapping_add(1);
    let flags = Flags {
        z: result == 0,
        c: flags.c,
        n: false,
        h: (a & 0x0F) + 1 > 0x0F,
    };
    (result, flags)
}

#[inline(always)]
fn xor(a: u8, b: u8) -> (u8, Flags) {
    let result = a ^ b;
    let flags = Flags {
        z: result == 0,
        n: false,
        h: false,
        c: false,
    };

    (result, flags)
}

#[inline(always)]
fn or(a: u8, b: u8) -> (u8, Flags) {
    // {"Z": "Z", "N": "0", "H": "0", "C": "0"}
    let result = a | b;
    let flags = Flags {
        z: result == 0,
        n: false,
        h: false,
        c: false,
    };
    (result, flags)
}

#[inline(always)]
fn cp(a: u8, b: u8) -> Flags {
    let result = a.wrapping_sub(b);

    Flags {
        z: result == 0,
        n: true,
        h: (a & 0x0F) < (b & 0x0F),
        c: a < b,
    }
}

#[inline(always)]
fn and(a: u8, b: u8) -> (u8, Flags) {
    //  flags: {"Z": "Z", "N": "0", "H": "1", "C": "0"}

    let result = a & b;

    let flags = Flags {
        n: false,
        c: false,
        h: true,
        z: result == 0,
    };

    (result, flags)
}

/// Rotate left through carry for an 8-bit value.
/// Returns the new value and updates the CPU flags.
/// If `set_z_flag` is false (e.g. for `RL A`), Z flag is not updated.
#[inline(always)]
fn rl(value: u8, flags: &Flags, set_z_flag: bool) -> (u8, Flags) {
    let bit7 = (value & 0x80) != 0;
    let carry_in = if flags.c { 1 } else { 0 };

    let result = (value << 1) | carry_in;

    let mut new_flags = flags.clone();
    // Update flags
    if set_z_flag {
        new_flags.z = result == 0;
    }
    new_flags.n = false;
    new_flags.h = false;
    new_flags.c = bit7;

    (result, new_flags)
}

#[inline(always)]
fn jr(pc: u16, offset: u8) -> u16 {
    let signed_offset = offset as i8;
    pc.wrapping_add(signed_offset as i16 as u16)
}

#[inline(always)]
fn rla(a: u8, flags: &Flags) -> (u8, Flags) {
    // Save old carry
    let old_carry = if flags.c { 1 } else { 0 };

    // Extract bit 7 (MSB) of A as new carry
    let new_carry = (a & 0x80) != 0;

    // Rotate left through carry
    let result = (a << 1) | old_carry;

    let new_flags = Flags {
        z: false,
        n: false,
        h: false,
        c: new_carry,
    };

    (result, new_flags)
}

#[inline(always)]
fn sub(a: u8, b: u8) -> (u8, Flags) {
    let result = a.wrapping_sub(b);

    let z = result == 0;
    let n = true; // subtraction
    let h = (a & 0x0F) < (b & 0x0F); // half-borrow from bit 4
    let c = a < b; // borrow (full carry)

    (result, Flags { z, n, h, c })
}

/// Perform an 8-bit addition like ADD A, r/imm/(HL).
/// Returns the new A value and the new flags.
#[inline(always)]
fn add(a: u8, val: u8) -> (u8, Flags) {
    let (res, carry) = a.overflowing_add(val);

    // Half carry: if adding the low nibbles produced a carry out of bit 3
    let half_carry = ((a & 0x0F) + (val & 0x0F)) > 0x0F;

    let flags = Flags {
        z: res == 0,
        n: false, // ADD clears N
        h: half_carry,
        c: carry,
    };

    (res, flags)
}

#[inline(always)]
fn rra(a: u8, carry_in: bool) -> (u8, Flags) {
    let new_c = (a & 0x01) != 0; // old bit0 becomes Carry
    let result = (a >> 1) | if carry_in { 0x80 } else { 0x00 }; // carry_in to bit7

    let flags = Flags {
        z: false, // RRA always clears Z on GB
        n: false,
        h: false,
        c: new_c,
    };
    (result, flags)
}
