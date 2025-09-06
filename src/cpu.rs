use crate::memory::{InterruptSource, Memory};
use crate::util::is_nth_bit_set;
use anyhow::Result;
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

#[derive(Default)]
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

#[derive(Default, Clone)]
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
            "a={:#x},b={:#x},c={:#x},d={:#x},e={:#x},h={:#x},l={:#x},sp={:#04x},pc={:#04x}",
            self.a, self.b, self.c, self.d, self.e, self.h, self.l, self.sp, self.pc
        )
    }
}

impl Cpu {
    fn fetch_imm8(&mut self, memory: &Memory) -> Result<u8> {
        let byte = memory.read(self.registers.pc)?;
        self.registers.pc = self.registers.pc.wrapping_add(1);
        Ok(byte)
    }

    fn fetch_imm16(&mut self, memory: &Memory) -> Result<u16> {
        let lo = self.fetch_imm8(memory)?;
        let hi = self.fetch_imm8(memory)?;
        let imm16 = join_u16(lo, hi);
        Ok(imm16)
    }

    /// Push a 16-bit value onto the stack (little-endian)
    fn push_16stk(&mut self, value: u16, memory: &mut Memory) -> Result<()> {
        let (lo, hi) = split_u16(value);
        self.registers.sp -= 1;
        memory.write(self.registers.sp, lo)?; // Push low byte
        self.registers.sp -= 1;
        memory.write(self.registers.sp, hi)?; // Push high byte
        Ok(())
    }

    /// Pop a 16-bit value from the stack (little-endian)
    fn pop_16stk(&mut self, memory: &Memory) -> Result<u16> {
        let hi = memory.read(self.registers.sp)?;
        self.registers.sp += 1;
        let lo = memory.read(self.registers.sp)?;
        self.registers.sp += 1;
        let value = join_u16(lo, hi);
        Ok(value)
    }

    pub fn service_interrupt(
        &mut self,
        interrupt: &InterruptSource,
        memory: &mut Memory,
    ) -> Result<u8> {
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
            self.push_16stk(self.registers.pc, memory)?;
            self.registers.pc = interrupt.jump_addres();
            self.ime = false;
        }

        Ok(5)
    }

    fn register_af(&self) -> u16 {
        let f = (self.flags.z as u8)
            + ((self.flags.n as u8) << 1)
            + ((self.flags.h as u8) << 2)
            + ((self.flags.c as u8) << 3);
        join_u16(self.registers.a, f)
    }

    /// return number of CPU T-cycles the step consumed
    //#[tracing::instrument(err, skip(memory))]
    pub fn step(&mut self, memory: &mut Memory) -> Result<u8> {
        if self.registers.pc == GAME_START {
            tracing::info!("Reached game start!");
        }

        if self.registers.pc == 0x00e0 {
            tracing::debug!("Nintendo logo verification");
        }

        let opcode = self.fetch_imm8(memory)?;
        // tracing::trace!("opcode = {:08b}", opcode);
        let n_cycles = match opcode {
            // codegen-start
            0x00 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("NOP ");
                4
            }
            0x01 => {
                // [{'name': 'BC', 'immediate': True}, {'name': 'n16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD BC, n16");
                let imm16 = self.fetch_imm16(memory)?;
                self.registers.set_bc(imm16);
                12
            }
            0x02 => {
                // [{'name': 'BC', 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD (BC), A");
                memory.write(self.registers.bc(), self.registers.a)?;
                8
            }
            0x03 => {
                // [{'name': 'BC', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("INC BC");
                anyhow::bail!("opcode INC BC not implemented")
                // cycles: [8]
            }
            0x04 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC B");
                (self.registers.b, self.flags) = inc(self.registers.b, &self.flags);
                4
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
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.b = imm8;
                8
            }
            0x07 => {
                // []
                // {'Z': '0', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLCA ");
                anyhow::bail!("opcode RLCA  not implemented")
                // cycles: [4]
            }
            0x08 => {
                // [{'name': 'a16', 'bytes': 2, 'immediate': False}, {'name': 'SP', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD a16, SP");
                anyhow::bail!("opcode LD a16, SP not implemented")
                // cycles: [20]
            }
            0x09 => {
                // [{'name': 'HL', 'immediate': True}, {'name': 'BC', 'immediate': True}]
                // {'Z': '-', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD HL, BC");
                anyhow::bail!("opcode ADD HL, BC not implemented")
                // cycles: [8]
            }
            0x0A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'BC', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, BC");
                anyhow::bail!("opcode LD A, BC not implemented")
                // cycles: [8]
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
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.c = imm8;
                8
            }
            0x0F => {
                // []
                // {'Z': '0', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRCA ");
                anyhow::bail!("opcode RRCA  not implemented")
                // cycles: [4]
            }
            0x10 => {
                // [{'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("STOP n8");
                anyhow::bail!("opcode STOP n8 not implemented")
                // cycles: [4]
            }
            0x11 => {
                // [{'name': 'DE', 'immediate': True}, {'name': 'n16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD DE, n16");
                let imm16 = self.fetch_imm16(memory)?;
                self.registers.set_de(imm16);
                12
            }
            0x12 => {
                // [{'name': 'DE', 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD DE, A");
                anyhow::bail!("opcode LD DE, A not implemented")
                // cycles: [8]
            }
            0x13 => {
                // [{'name': 'DE', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("INC DE");
                self.registers.set_de(self.registers.de() + 1);
                8
            }
            0x14 => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC D");
                anyhow::bail!("opcode INC D not implemented")
                // cycles: [4]
            }
            0x15 => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC D");
                (self.registers.d, self.flags) = dec(self.registers.d, &self.flags);
                4
            }
            0x16 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, n8");
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.d = imm8;
                8
            }
            0x17 => {
                // []
                // {'Z': '0', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLA ");
                (self.registers.a, self.flags) = rla(self.registers.a, &self.flags);
                4
            }
            0x18 => {
                // [{'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JR e8");
                let offset = self.fetch_imm8(memory)?;
                self.registers.pc = jr(self.registers.pc, offset);
                12
            }
            0x19 => {
                // [{'name': 'HL', 'immediate': True}, {'name': 'DE', 'immediate': True}]
                // {'Z': '-', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD HL, DE");
                anyhow::bail!("opcode ADD HL, DE not implemented")
                // cycles: [8]
            }
            0x1A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'DE', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, (DE)");
                let a = memory.read(self.registers.de())?;
                self.registers.a = a;
                8
            }
            0x1B => {
                // [{'name': 'DE', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("DEC DE");
                anyhow::bail!("opcode DEC DE not implemented")
                // cycles: [8]
            }
            0x1C => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC E");
                (self.registers.e, self.flags) = inc(self.registers.e, &self.flags);
                4
            }
            0x1D => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC E");
                (self.registers.e, self.flags) = dec(self.registers.e, &self.flags);
                4
            }
            0x1E => {
                // [{'name': 'E', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, n8");
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.e = imm8;
                8
            }
            0x1F => {
                // []
                // {'Z': '0', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRA ");
                (self.registers.a, self.flags) = rra(self.registers.a, self.flags.c);
                4
            }
            0x20 => {
                // [{'name': 'NZ', 'immediate': True}, {'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JR NZ, e8");
                let offset = self.fetch_imm8(memory)?;

                if !self.flags.z {
                    self.registers.pc = jr(self.registers.pc, offset);
                    12
                } else {
                    8
                }
            }
            0x21 => {
                // [{'name': 'HL', 'immediate': True}, {'name': 'n16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, n16");
                let imm16 = self.fetch_imm16(memory)?;
                self.registers.set_hl(imm16);
                12
            }
            0x22 => {
                // [{'name': 'HL', 'increment': True, 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, A");
                memory.write(self.registers.hl(), self.registers.a)?;
                self.registers.set_hl(self.registers.hl() + 1);
                8
            }
            0x23 => {
                // [{'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
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
            0x25 => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC H");
                anyhow::bail!("opcode DEC H not implemented")
                // cycles: [4]
            }
            0x26 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, n8");
                anyhow::bail!("opcode LD H, n8 not implemented")
                // cycles: [8]
            }
            0x27 => {
                // []
                // {'Z': 'Z', 'N': '-', 'H': '0', 'C': 'C'}
                tracing::trace!("DAA ");
                anyhow::bail!("opcode DAA  not implemented")
                // cycles: [4]
            }
            0x28 => {
                // [{'name': 'Z', 'immediate': True}, {'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JR Z, e8");
                let offset = self.fetch_imm8(memory)?;
                if self.flags.z {
                    self.registers.pc = jr(self.registers.pc, offset);
                    12
                } else {
                    8
                }
            }
            0x29 => {
                // [{'name': 'HL', 'immediate': True}, {'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD HL, HL");
                anyhow::bail!("opcode ADD HL, HL not implemented")
                // cycles: [8]
            }
            0x2A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'increment': True, 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, (HL)+");
                self.registers.a = memory.read(self.registers.hl())?;
                self.registers.set_hl(self.registers.hl() + 1);
                8
            }
            0x2B => {
                // [{'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
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
            0x2D => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC L");
                anyhow::bail!("opcode DEC L not implemented")
                // cycles: [4]
            }
            0x2E => {
                // [{'name': 'L', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, n8");
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.l = imm8;
                8
            }
            0x2F => {
                // []
                // {'Z': '-', 'N': '1', 'H': '1', 'C': '-'}
                tracing::trace!("CPL ");
                anyhow::bail!("opcode CPL  not implemented")
                // cycles: [4]
            }
            0x30 => {
                // [{'name': 'NC', 'immediate': True}, {'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JR NC, e8");
                anyhow::bail!("opcode JR NC, e8 not implemented")
                // cycles: [12, 8]
            }
            0x31 => {
                // [{'name': 'SP', 'immediate': True}, {'name': 'n16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD SP, n16");
                let imm16 = self.fetch_imm16(memory)?;
                self.registers.sp = imm16;
                12
            }
            0x32 => {
                // [{'name': 'HL', 'decrement': True, 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD (HL)-, A");
                memory.write(self.registers.hl(), self.registers.a)?;
                self.registers.set_hl(self.registers.hl() - 1);
                8
            }
            0x33 => {
                // [{'name': 'SP', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("INC SP");
                anyhow::bail!("opcode INC SP not implemented")
                // cycles: [8]
            }
            0x34 => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC HL");
                anyhow::bail!("opcode INC HL not implemented")
                // cycles: [12]
            }
            0x35 => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC HL");
                anyhow::bail!("opcode DEC HL not implemented")
                // cycles: [12]
            }
            0x36 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD (HL), n8");
                let imm8 = self.fetch_imm8(memory)?;
                memory.write(self.registers.hl(), imm8)?;
                12
            }
            0x37 => {
                // []
                // {'Z': '-', 'N': '0', 'H': '0', 'C': '1'}
                tracing::trace!("SCF ");
                self.flags.c = true;
                self.flags.n = false;
                self.flags.h = false;
                4
            }
            0x38 => {
                // [{'name': 'C', 'immediate': True}, {'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JR C, e8");
                anyhow::bail!("opcode JR C, e8 not implemented")
                // cycles: [12, 8]
            }
            0x39 => {
                // [{'name': 'HL', 'immediate': True}, {'name': 'SP', 'immediate': True}]
                // {'Z': '-', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD HL, SP");
                anyhow::bail!("opcode ADD HL, SP not implemented")
                // cycles: [8]
            }
            0x3A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'decrement': True, 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, HL");
                anyhow::bail!("opcode LD A, HL not implemented")
                // cycles: [8]
            }
            0x3B => {
                // [{'name': 'SP', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("DEC SP");
                anyhow::bail!("opcode DEC SP not implemented")
                // cycles: [8]
            }
            0x3C => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': '-'}
                tracing::trace!("INC A");
                anyhow::bail!("opcode INC A not implemented")
                // cycles: [4]
            }
            0x3D => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("DEC A");
                (self.registers.a, self.flags) = dec(self.registers.a, &self.flags);
                4
            }
            0x3E => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, n8");
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.a = imm8;
                8
            }
            0x3F => {
                // []
                // {'Z': '-', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("CCF ");
                anyhow::bail!("opcode CCF  not implemented")
                // cycles: [4]
            }
            0x40 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, B");
                anyhow::bail!("opcode LD B, B not implemented")
                // cycles: [4]
            }
            0x41 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, C");
                anyhow::bail!("opcode LD B, C not implemented")
                // cycles: [4]
            }
            0x42 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, D");
                anyhow::bail!("opcode LD B, D not implemented")
                // cycles: [4]
            }
            0x43 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, E");
                anyhow::bail!("opcode LD B, E not implemented")
                // cycles: [4]
            }
            0x44 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, H");
                anyhow::bail!("opcode LD B, H not implemented")
                // cycles: [4]
            }
            0x45 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, L");
                anyhow::bail!("opcode LD B, L not implemented")
                // cycles: [4]
            }
            0x46 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, HL");
                anyhow::bail!("opcode LD B, HL not implemented")
                // cycles: [8]
            }
            0x47 => {
                // [{'name': 'B', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD B, A");
                anyhow::bail!("opcode LD B, A not implemented")
                // cycles: [4]
            }
            0x48 => {
                // [{'name': 'C', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, B");
                anyhow::bail!("opcode LD C, B not implemented")
                // cycles: [4]
            }
            0x49 => {
                // [{'name': 'C', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, C");
                4
            }
            0x4A => {
                // [{'name': 'C', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, D");
                anyhow::bail!("opcode LD C, D not implemented")
                // cycles: [4]
            }
            0x4B => {
                // [{'name': 'C', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, E");
                anyhow::bail!("opcode LD C, E not implemented")
                // cycles: [4]
            }
            0x4C => {
                // [{'name': 'C', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, H");
                anyhow::bail!("opcode LD C, H not implemented")
                // cycles: [4]
            }
            0x4D => {
                // [{'name': 'C', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, L");
                anyhow::bail!("opcode LD C, L not implemented")
                // cycles: [4]
            }
            0x4E => {
                // [{'name': 'C', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, HL");
                anyhow::bail!("opcode LD C, HL not implemented")
                // cycles: [8]
            }
            0x4F => {
                // [{'name': 'C', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD C, A");
                self.registers.c = self.registers.a;
                4
            }
            0x50 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, B");
                anyhow::bail!("opcode LD D, B not implemented")
                // cycles: [4]
            }
            0x51 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, C");
                anyhow::bail!("opcode LD D, C not implemented")
                // cycles: [4]
            }
            0x52 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, D");
                anyhow::bail!("opcode LD D, D not implemented")
                // cycles: [4]
            }
            0x53 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, E");
                anyhow::bail!("opcode LD D, E not implemented")
                // cycles: [4]
            }
            0x54 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, H");
                anyhow::bail!("opcode LD D, H not implemented")
                // cycles: [4]
            }
            0x55 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, L");
                anyhow::bail!("opcode LD D, L not implemented")
                // cycles: [4]
            }
            0x56 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, HL");
                anyhow::bail!("opcode LD D, HL not implemented")
                // cycles: [8]
            }
            0x57 => {
                // [{'name': 'D', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD D, A");
                self.registers.d = self.registers.a;
                4
            }
            0x58 => {
                // [{'name': 'E', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, B");
                anyhow::bail!("opcode LD E, B not implemented")
                // cycles: [4]
            }
            0x59 => {
                // [{'name': 'E', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, C");
                anyhow::bail!("opcode LD E, C not implemented")
                // cycles: [4]
            }
            0x5A => {
                // [{'name': 'E', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, D");
                anyhow::bail!("opcode LD E, D not implemented")
                // cycles: [4]
            }
            0x5B => {
                // [{'name': 'E', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, E");
                anyhow::bail!("opcode LD E, E not implemented")
                // cycles: [4]
            }
            0x5C => {
                // [{'name': 'E', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, H");
                anyhow::bail!("opcode LD E, H not implemented")
                // cycles: [4]
            }
            0x5D => {
                // [{'name': 'E', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, L");
                anyhow::bail!("opcode LD E, L not implemented")
                // cycles: [4]
            }
            0x5E => {
                // [{'name': 'E', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, HL");
                anyhow::bail!("opcode LD E, HL not implemented")
                // cycles: [8]
            }
            0x5F => {
                // [{'name': 'E', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD E, A");
                anyhow::bail!("opcode LD E, A not implemented")
                // cycles: [4]
            }
            0x60 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, B");
                anyhow::bail!("opcode LD H, B not implemented")
                // cycles: [4]
            }
            0x61 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, C");
                anyhow::bail!("opcode LD H, C not implemented")
                // cycles: [4]
            }
            0x62 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, D");
                anyhow::bail!("opcode LD H, D not implemented")
                // cycles: [4]
            }
            0x63 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, E");
                anyhow::bail!("opcode LD H, E not implemented")
                // cycles: [4]
            }
            0x64 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, H");
                anyhow::bail!("opcode LD H, H not implemented")
                // cycles: [4]
            }
            0x65 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, L");
                anyhow::bail!("opcode LD H, L not implemented")
                // cycles: [4]
            }
            0x66 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, HL");
                anyhow::bail!("opcode LD H, HL not implemented")
                // cycles: [8]
            }
            0x67 => {
                // [{'name': 'H', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD H, A");
                self.registers.h = self.registers.a;
                4
            }
            0x68 => {
                // [{'name': 'L', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, B");
                anyhow::bail!("opcode LD L, B not implemented")
                // cycles: [4]
            }
            0x69 => {
                // [{'name': 'L', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, C");
                anyhow::bail!("opcode LD L, C not implemented")
                // cycles: [4]
            }
            0x6A => {
                // [{'name': 'L', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, D");
                anyhow::bail!("opcode LD L, D not implemented")
                // cycles: [4]
            }
            0x6B => {
                // [{'name': 'L', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, E");
                anyhow::bail!("opcode LD L, E not implemented")
                // cycles: [4]
            }
            0x6C => {
                // [{'name': 'L', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, H");
                anyhow::bail!("opcode LD L, H not implemented")
                // cycles: [4]
            }
            0x6D => {
                // [{'name': 'L', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, L");
                anyhow::bail!("opcode LD L, L not implemented")
                // cycles: [4]
            }
            0x6E => {
                // [{'name': 'L', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, HL");
                anyhow::bail!("opcode LD L, HL not implemented")
                // cycles: [8]
            }
            0x6F => {
                // [{'name': 'L', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD L, A");
                anyhow::bail!("opcode LD L, A not implemented")
                // cycles: [4]
            }
            0x70 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, B");
                anyhow::bail!("opcode LD HL, B not implemented")
                // cycles: [8]
            }
            0x71 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, C");
                anyhow::bail!("opcode LD HL, C not implemented")
                // cycles: [8]
            }
            0x72 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, D");
                anyhow::bail!("opcode LD HL, D not implemented")
                // cycles: [8]
            }
            0x73 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, E");
                anyhow::bail!("opcode LD HL, E not implemented")
                // cycles: [8]
            }
            0x74 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, H");
                anyhow::bail!("opcode LD HL, H not implemented")
                // cycles: [8]
            }
            0x75 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD HL, L");
                anyhow::bail!("opcode LD HL, L not implemented")
                // cycles: [8]
            }
            0x76 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("HALT ");
                anyhow::bail!("opcode HALT  not implemented")
                // cycles: [4]
            }
            0x77 => {
                // [{'name': 'HL', 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD (HL), A");
                memory.write(self.registers.hl(), self.registers.a)?;
                8
            }
            0x78 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, B");
                self.registers.a = self.registers.b;
                4
            }
            0x79 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, C");
                anyhow::bail!("opcode LD A, C not implemented")
                // cycles: [4]
            }
            0x7A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, D");
                anyhow::bail!("opcode LD A, D not implemented")
                // cycles: [4]
            }
            0x7B => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, E");
                self.registers.a = self.registers.e;
                4
            }
            0x7C => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, H");
                self.registers.a = self.registers.h;
                4
            }
            0x7D => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, L");
                self.registers.a = self.registers.l;
                4
            }
            0x7E => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, (HL)");
                self.registers.a = memory.read(self.registers.hl())?;
                8
            }
            0x7F => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, A");
                4
            }
            0x80 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, B");
                anyhow::bail!("opcode ADD A, B not implemented")
                // cycles: [4]
            }
            0x81 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, C");
                anyhow::bail!("opcode ADD A, C not implemented")
                // cycles: [4]
            }
            0x82 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, D");
                anyhow::bail!("opcode ADD A, D not implemented")
                // cycles: [4]
            }
            0x83 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, E");
                (self.registers.a, self.flags) = add(self.registers.a, self.registers.e);
                4
            }
            0x84 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, H");
                anyhow::bail!("opcode ADD A, H not implemented")
                // cycles: [4]
            }
            0x85 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, L");
                anyhow::bail!("opcode ADD A, L not implemented")
                // cycles: [4]
            }
            0x86 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, (HL)");
                let value = memory.read(self.registers.hl())?;
                (self.registers.a, self.flags) = add(self.registers.a, value);
                8
            }
            0x87 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, A");
                anyhow::bail!("opcode ADD A, A not implemented")
                // cycles: [4]
            }
            0x88 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, B");
                anyhow::bail!("opcode ADC A, B not implemented")
                // cycles: [4]
            }
            0x89 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, C");
                (self.registers.a, self.flags) = adc(self.registers.a, self.registers.c);
                4
            }
            0x8A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, D");
                anyhow::bail!("opcode ADC A, D not implemented")
                // cycles: [4]
            }
            0x8B => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, E");
                anyhow::bail!("opcode ADC A, E not implemented")
                // cycles: [4]
            }
            0x8C => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, H");
                anyhow::bail!("opcode ADC A, H not implemented")
                // cycles: [4]
            }
            0x8D => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, L");
                anyhow::bail!("opcode ADC A, L not implemented")
                // cycles: [4]
            }
            0x8E => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, HL");
                anyhow::bail!("opcode ADC A, HL not implemented")
                // cycles: [8]
            }
            0x8F => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, A");
                anyhow::bail!("opcode ADC A, A not implemented")
                // cycles: [4]
            }
            0x90 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, B");
                (self.registers.a, self.flags) = sub(self.registers.a, self.registers.b);
                4
            }
            0x91 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, C");
                anyhow::bail!("opcode SUB A, C not implemented")
                // cycles: [4]
            }
            0x92 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, D");
                anyhow::bail!("opcode SUB A, D not implemented")
                // cycles: [4]
            }
            0x93 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, E");
                anyhow::bail!("opcode SUB A, E not implemented")
                // cycles: [4]
            }
            0x94 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, H");
                anyhow::bail!("opcode SUB A, H not implemented")
                // cycles: [4]
            }
            0x95 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, L");
                anyhow::bail!("opcode SUB A, L not implemented")
                // cycles: [4]
            }
            0x96 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, (HL)");
                let addr = self.registers.hl();
                let value = memory.read(addr)?;
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
            0x98 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, B");
                anyhow::bail!("opcode SBC A, B not implemented")
                // cycles: [4]
            }
            0x99 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, C");
                anyhow::bail!("opcode SBC A, C not implemented")
                // cycles: [4]
            }
            0x9A => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, D");
                anyhow::bail!("opcode SBC A, D not implemented")
                // cycles: [4]
            }
            0x9B => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, E");
                anyhow::bail!("opcode SBC A, E not implemented")
                // cycles: [4]
            }
            0x9C => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, H");
                anyhow::bail!("opcode SBC A, H not implemented")
                // cycles: [4]
            }
            0x9D => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, L");
                anyhow::bail!("opcode SBC A, L not implemented")
                // cycles: [4]
            }
            0x9E => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, HL");
                anyhow::bail!("opcode SBC A, HL not implemented")
                // cycles: [8]
            }
            0x9F => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': '-'}
                tracing::trace!("SBC A, A");
                anyhow::bail!("opcode SBC A, A not implemented")
                // cycles: [4]
            }
            0xA0 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, B");
                anyhow::bail!("opcode AND A, B not implemented")
                // cycles: [4]
            }
            0xA1 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, C");
                (self.registers.a, self.flags) = and(self.registers.a, self.registers.c);
                4
            }
            0xA2 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, D");
                anyhow::bail!("opcode AND A, D not implemented")
                // cycles: [4]
            }
            0xA3 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, E");
                anyhow::bail!("opcode AND A, E not implemented")
                // cycles: [4]
            }
            0xA4 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, H");
                anyhow::bail!("opcode AND A, H not implemented")
                // cycles: [4]
            }
            0xA5 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, L");
                anyhow::bail!("opcode AND A, L not implemented")
                // cycles: [4]
            }
            0xA6 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, HL");
                anyhow::bail!("opcode AND A, HL not implemented")
                // cycles: [8]
            }
            0xA7 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, A");
                (self.registers.a, self.flags) = and(self.registers.a, self.registers.a);
                4
            }
            0xA8 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, B");
                anyhow::bail!("opcode XOR A, B not implemented")
                // cycles: [4]
            }
            0xA9 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, C");
                anyhow::bail!("opcode XOR A, C not implemented")
                // cycles: [4]
            }
            0xAA => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, D");
                anyhow::bail!("opcode XOR A, D not implemented")
                // cycles: [4]
            }
            0xAB => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, E");
                anyhow::bail!("opcode XOR A, E not implemented")
                // cycles: [4]
            }
            0xAC => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, H");
                anyhow::bail!("opcode XOR A, H not implemented")
                // cycles: [4]
            }
            0xAD => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, L");
                anyhow::bail!("opcode XOR A, L not implemented")
                // cycles: [4]
            }
            0xAE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, HL");
                anyhow::bail!("opcode XOR A, HL not implemented")
                // cycles: [8]
            }
            0xAF => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '1', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, A");
                (self.registers.a, self.flags) = xor(self.registers.a, self.registers.a);
                4
            }
            0xB0 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, B");
                anyhow::bail!("opcode OR A, B not implemented")
                // cycles: [4]
            }
            0xB1 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, C");
                (self.registers.a, self.flags) = or(self.registers.a, self.registers.c);
                4
            }
            0xB2 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, D");
                anyhow::bail!("opcode OR A, D not implemented")
                // cycles: [4]
            }
            0xB3 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, E");
                anyhow::bail!("opcode OR A, E not implemented")
                // cycles: [4]
            }
            0xB4 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, H");
                anyhow::bail!("opcode OR A, H not implemented")
                // cycles: [4]
            }
            0xB5 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, L");
                (self.registers.a, self.flags) = or(self.registers.a, self.registers.l);
                4
            }
            0xB6 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, HL");
                anyhow::bail!("opcode OR A, HL not implemented")
                // cycles: [8]
            }
            0xB7 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, A");
                anyhow::bail!("opcode OR A, A not implemented")
                // cycles: [4]
            }
            0xB8 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, B");
                anyhow::bail!("opcode CP A, B not implemented")
                // cycles: [4]
            }
            0xB9 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, C");
                anyhow::bail!("opcode CP A, C not implemented")
                // cycles: [4]
            }
            0xBA => {
                // [{'name': 'A', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, D");
                anyhow::bail!("opcode CP A, D not implemented")
                // cycles: [4]
            }
            0xBB => {
                // [{'name': 'A', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, E");
                anyhow::bail!("opcode CP A, E not implemented")
                // cycles: [4]
            }
            0xBC => {
                // [{'name': 'A', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, H");
                anyhow::bail!("opcode CP A, H not implemented")
                // cycles: [4]
            }
            0xBD => {
                // [{'name': 'A', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, L");
                anyhow::bail!("opcode CP A, L not implemented")
                // cycles: [4]
            }
            0xBE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, (HL)");
                self.flags = cp(self.registers.a, memory.read(self.registers.hl())?);
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
                // [{'name': 'NZ', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RET NZ");
                if !self.flags.z {
                    let addr = self.pop_16stk(memory)?;
                    self.registers.pc = addr;
                    20
                } else {
                    8
                }
            }
            0xC1 => {
                // [{'name': 'BC', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("POP BC");
                let value = self.pop_16stk(memory)?;
                self.registers.set_bc(value);
                12
            }
            0xC2 => {
                // [{'name': 'NZ', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JP NZ, a16");
                anyhow::bail!("opcode JP NZ, a16 not implemented")
                // cycles: [16, 12]
            }
            0xC3 => {
                // [{'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JP a16");
                let imm16 = self.fetch_imm16(memory)?;
                self.registers.pc = imm16;
                16
            }
            0xC4 => {
                // [{'name': 'NZ', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("CALL NZ, a16");
                anyhow::bail!("opcode CALL NZ, a16 not implemented")
                // cycles: [24, 12]
            }
            0xC5 => {
                // [{'name': 'BC', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("PUSH BC");
                self.push_16stk(self.registers.bc(), memory)?;
                16
            }
            0xC6 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD A, n8");
                anyhow::bail!("opcode ADD A, n8 not implemented")
                // cycles: [8]
            }
            0xC7 => {
                // [{'name': '$00', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $00");
                anyhow::bail!("opcode RST $00 not implemented")
                // cycles: [16]
            }
            0xC8 => {
                // [{'name': 'Z', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RET Z");
                anyhow::bail!("opcode RET Z not implemented")
                // cycles: [20, 8]
            }
            0xC9 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RET ");
                let ret_addr = self.pop_16stk(memory)?;
                self.registers.pc = ret_addr;
                16
            }
            0xCA => {
                // [{'name': 'Z', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JP Z, a16");
                anyhow::bail!("opcode JP Z, a16 not implemented")
                // cycles: [16, 12]
            }
            0xCB => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("PREFIX");
                let prefix_cycles = self.step_prefixed(memory)?;
                4 + prefix_cycles
            }
            0xCC => {
                // [{'name': 'Z', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("CALL Z, a16");
                anyhow::bail!("opcode CALL Z, a16 not implemented")
                // cycles: [24, 12]
            }
            0xCD => {
                // [{'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("CALL a16");
                let imm16 = self.fetch_imm16(memory)?;
                let return_addr = self.registers.pc + 1;
                self.push_16stk(return_addr, memory)?;
                self.registers.pc = imm16;
                24
            }
            0xCE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADC A, n8");
                anyhow::bail!("opcode ADC A, n8 not implemented")
                // cycles: [8]
            }
            0xCF => {
                // [{'name': '$08', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $08");
                anyhow::bail!("opcode RST $08 not implemented")
                // cycles: [16]
            }
            0xD0 => {
                // [{'name': 'NC', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RET NC");
                anyhow::bail!("opcode RET NC not implemented")
                // cycles: [20, 8]
            }
            0xD1 => {
                // [{'name': 'DE', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("POP DE");
                anyhow::bail!("opcode POP DE not implemented")
                // cycles: [12]
            }
            0xD2 => {
                // [{'name': 'NC', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JP NC, a16");
                anyhow::bail!("opcode JP NC, a16 not implemented")
                // cycles: [16, 12]
            }
            0xD3 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_D3 ");
                anyhow::bail!("opcode ILLEGAL_D3  not implemented")
                // cycles: [4]
            }
            0xD4 => {
                // [{'name': 'NC', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("CALL NC, a16");
                anyhow::bail!("opcode CALL NC, a16 not implemented")
                // cycles: [24, 12]
            }
            0xD5 => {
                // [{'name': 'DE', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("PUSH DE");
                self.push_16stk(self.registers.de(), memory)?;
                16
            }
            0xD6 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SUB A, n8");
                anyhow::bail!("opcode SUB A, n8 not implemented")
                // cycles: [8]
            }
            0xD7 => {
                // [{'name': '$10', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $10");
                anyhow::bail!("opcode RST $10 not implemented")
                // cycles: [16]
            }
            0xD8 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RET C");
                anyhow::bail!("opcode RET C not implemented")
                // cycles: [20, 8]
            }
            0xD9 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RETI ");
                anyhow::bail!("opcode RETI  not implemented")
                // cycles: [16]
            }
            0xDA => {
                // [{'name': 'C', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JP C, a16");
                anyhow::bail!("opcode JP C, a16 not implemented")
                // cycles: [16, 12]
            }
            0xDB => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_DB ");
                anyhow::bail!("opcode ILLEGAL_DB  not implemented")
                // cycles: [4]
            }
            0xDC => {
                // [{'name': 'C', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("CALL C, a16");
                anyhow::bail!("opcode CALL C, a16 not implemented")
                // cycles: [24, 12]
            }
            0xDD => {
                tracing::trace!("ILLEGAL_DD");
                4
            }
            0xDE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("SBC A, n8");
                anyhow::bail!("opcode SBC A, n8 not implemented")
                // cycles: [8]
            }
            0xDF => {
                // [{'name': '$18', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $18");
                anyhow::bail!("opcode RST $18 not implemented")
                // cycles: [16]
            }
            0xE0 => {
                // [{'name': 'a8', 'bytes': 1, 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LDH a8, A");
                let imm8 = self.fetch_imm8(memory)?;
                memory.write(imm8.high_addr(), self.registers.a)?;
                12
            }
            0xE1 => {
                // [{'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("POP HL");
                anyhow::bail!("opcode POP HL not implemented")
                // cycles: [12]
            }
            0xE2 => {
                // [{'name': 'C', 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LDH [C], A");
                memory.write(self.registers.c.high_addr(), self.registers.a)?;
                8
            }
            0xE3 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_E3 ");
                anyhow::bail!("opcode ILLEGAL_E3  not implemented")
                // cycles: [4]
            }
            0xE4 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_E4 ");
                anyhow::bail!("opcode ILLEGAL_E4  not implemented")
                // cycles: [4]
            }
            0xE5 => {
                // [{'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("PUSH HL");
                self.push_16stk(self.registers.hl(), memory)?;
                16
            }
            0xE6 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '0'}
                tracing::trace!("AND A, n8");
                anyhow::bail!("opcode AND A, n8 not implemented")
                // cycles: [8]
            }
            0xE7 => {
                // [{'name': '$20', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $20");
                anyhow::bail!("opcode RST $20 not implemented")
                // cycles: [16]
            }
            0xE8 => {
                // [{'name': 'SP', 'immediate': True}, {'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '0', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("ADD SP, e8");
                anyhow::bail!("opcode ADD SP, e8 not implemented")
                // cycles: [16]
            }
            0xE9 => {
                // [{'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("JP HL");
                anyhow::bail!("opcode JP HL not implemented")
                // cycles: [4]
            }
            0xEA => {
                // [{'name': 'a16', 'bytes': 2, 'immediate': False}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD (a16), A");
                let addr = self.fetch_imm16(memory)?;
                memory.write(addr, self.registers.a)?;
                16
            }
            0xEB => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_EB ");
                anyhow::bail!("opcode ILLEGAL_EB  not implemented")
                // cycles: [4]
            }
            0xEC => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_EC ");
                anyhow::bail!("opcode ILLEGAL_EC  not implemented")
                // cycles: [4]
            }
            0xED => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_ED ");
                anyhow::bail!("opcode ILLEGAL_ED  not implemented")
                // cycles: [4]
            }
            0xEE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("XOR A, n8");
                anyhow::bail!("opcode XOR A, n8 not implemented")
                // cycles: [8]
            }
            0xEF => {
                // [{'name': '$28', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $28");
                anyhow::bail!("opcode RST $28 not implemented")
                // cycles: [16]
            }
            0xF0 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'a8', 'bytes': 1, 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LDH A, a8");
                if self.registers.pc == 0x65 {
                    tracing::debug!("Waiting for screen frame LY = {:#x}", memory.registers.ly);
                }
                let imm8 = self.fetch_imm8(memory)?;
                self.registers.a = memory.read(imm8.high_addr())?;
                12
            }
            0xF1 => {
                // [{'name': 'AF', 'immediate': True}]
                // {'Z': 'Z', 'N': 'N', 'H': 'H', 'C': 'C'}
                tracing::trace!("POP AF");
                anyhow::bail!("opcode POP AF not implemented")
                // cycles: [12]
            }
            0xF2 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'C', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LDH A, C");
                anyhow::bail!("opcode LDH A, C not implemented")
                // cycles: [8]
            }
            0xF3 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("DI");
                self.ime = false;
                4
            }
            0xF4 => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_F4 ");
                anyhow::bail!("opcode ILLEGAL_F4  not implemented")
                // cycles: [4]
            }
            0xF5 => {
                // [{'name': 'AF', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("PUSH AF");
                self.push_16stk(self.register_af(), memory)?;
                16
            }
            0xF6 => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("OR A, n8");
                anyhow::bail!("opcode OR A, n8 not implemented")
                // cycles: [8]
            }
            0xF7 => {
                // [{'name': '$30', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $30");
                anyhow::bail!("opcode RST $30 not implemented")
                // cycles: [16]
            }
            0xF8 => {
                // [{'name': 'HL', 'immediate': True}, {'name': 'SP', 'increment': True, 'immediate': True}, {'name': 'e8', 'bytes': 1, 'immediate': True}]
                // {'Z': '0', 'N': '0', 'H': 'H', 'C': 'C'}
                tracing::trace!("LD HL, SP, e8");
                anyhow::bail!("opcode LD HL, SP, e8 not implemented")
                // cycles: [12]
            }
            0xF9 => {
                // [{'name': 'SP', 'immediate': True}, {'name': 'HL', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD SP, HL");
                anyhow::bail!("opcode LD SP, HL not implemented")
                // cycles: [8]
            }
            0xFA => {
                // [{'name': 'A', 'immediate': True}, {'name': 'a16', 'bytes': 2, 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("LD A, a16");
                anyhow::bail!("opcode LD A, a16 not implemented")
                // cycles: [16]
            }
            0xFB => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("EI ");
                self.ime = true;
                4
            }
            0xFC => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_FC ");
                anyhow::bail!("opcode ILLEGAL_FC  not implemented")
                // cycles: [4]
            }
            0xFD => {
                // []
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("ILLEGAL_FD ");
                anyhow::bail!("opcode ILLEGAL_FD  not implemented")
                // cycles: [4]
            }
            0xFE => {
                // [{'name': 'A', 'immediate': True}, {'name': 'n8', 'bytes': 1, 'immediate': True}]
                // {'Z': 'Z', 'N': '1', 'H': 'H', 'C': 'C'}
                tracing::trace!("CP A, n8");
                let imm8 = self.fetch_imm8(memory)?;
                self.flags = cp(self.registers.a, imm8);
                8
            }
            0xFF => {
                // [{'name': '$38', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RST $38");
                anyhow::bail!("opcode RST $38 not implemented")
                // cycles: [16]
            } // codegen-end
              // _ => anyhow::bail!("Not implemented opcode={:08b}", opcode),
        };

        Ok(n_cycles)
    }

    // #[tracing::instrument(err, skip(memory))]
    fn step_prefixed(&mut self, memory: &mut Memory) -> Result<u8> {
        let opcode = self.fetch_imm8(memory)?;
        // tracing::trace!("prefix_opcode = {:08b}", opcode);
        let n_cycles = match opcode {
            // codegen-prefix-start
            0x00 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC B");
                anyhow::bail!("opcode RLC B not implemented")
                // cycles: [8]
            }
            0x01 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC C");
                anyhow::bail!("opcode RLC C not implemented")
                // cycles: [8]
            }
            0x02 => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC D");
                anyhow::bail!("opcode RLC D not implemented")
                // cycles: [8]
            }
            0x03 => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC E");
                anyhow::bail!("opcode RLC E not implemented")
                // cycles: [8]
            }
            0x04 => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC H");
                anyhow::bail!("opcode RLC H not implemented")
                // cycles: [8]
            }
            0x05 => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC L");
                anyhow::bail!("opcode RLC L not implemented")
                // cycles: [8]
            }
            0x06 => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC HL");
                anyhow::bail!("opcode RLC HL not implemented")
                // cycles: [16]
            }
            0x07 => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RLC A");
                anyhow::bail!("opcode RLC A not implemented")
                // cycles: [8]
            }
            0x08 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC B");
                anyhow::bail!("opcode RRC B not implemented")
                // cycles: [8]
            }
            0x09 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC C");
                anyhow::bail!("opcode RRC C not implemented")
                // cycles: [8]
            }
            0x0A => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC D");
                anyhow::bail!("opcode RRC D not implemented")
                // cycles: [8]
            }
            0x0B => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC E");
                anyhow::bail!("opcode RRC E not implemented")
                // cycles: [8]
            }
            0x0C => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC H");
                anyhow::bail!("opcode RRC H not implemented")
                // cycles: [8]
            }
            0x0D => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC L");
                anyhow::bail!("opcode RRC L not implemented")
                // cycles: [8]
            }
            0x0E => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC HL");
                anyhow::bail!("opcode RRC HL not implemented")
                // cycles: [16]
            }
            0x0F => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RRC A");
                anyhow::bail!("opcode RRC A not implemented")
                // cycles: [8]
            }
            0x10 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL B");
                anyhow::bail!("opcode RL B not implemented")
                // cycles: [8]
            }
            0x11 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL C");
                (self.registers.a, self.flags) = rl(self.registers.c, &self.flags, true);
                8
            }
            0x12 => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL D");
                anyhow::bail!("opcode RL D not implemented")
                // cycles: [8]
            }
            0x13 => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL E");
                anyhow::bail!("opcode RL E not implemented")
                // cycles: [8]
            }
            0x14 => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL H");
                anyhow::bail!("opcode RL H not implemented")
                // cycles: [8]
            }
            0x15 => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL L");
                anyhow::bail!("opcode RL L not implemented")
                // cycles: [8]
            }
            0x16 => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL HL");
                anyhow::bail!("opcode RL HL not implemented")
                // cycles: [16]
            }
            0x17 => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RL A");
                anyhow::bail!("opcode RL A not implemented")
                // cycles: [8]
            }
            0x18 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR B");
                anyhow::bail!("opcode RR B not implemented")
                // cycles: [8]
            }
            0x19 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR C");
                anyhow::bail!("opcode RR C not implemented")
                // cycles: [8]
            }
            0x1A => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR D");
                anyhow::bail!("opcode RR D not implemented")
                // cycles: [8]
            }
            0x1B => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR E");
                anyhow::bail!("opcode RR E not implemented")
                // cycles: [8]
            }
            0x1C => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR H");
                anyhow::bail!("opcode RR H not implemented")
                // cycles: [8]
            }
            0x1D => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR L");
                anyhow::bail!("opcode RR L not implemented")
                // cycles: [8]
            }
            0x1E => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR HL");
                anyhow::bail!("opcode RR HL not implemented")
                // cycles: [16]
            }
            0x1F => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("RR A");
                anyhow::bail!("opcode RR A not implemented")
                // cycles: [8]
            }
            0x20 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA B");
                anyhow::bail!("opcode SLA B not implemented")
                // cycles: [8]
            }
            0x21 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA C");
                anyhow::bail!("opcode SLA C not implemented")
                // cycles: [8]
            }
            0x22 => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA D");
                anyhow::bail!("opcode SLA D not implemented")
                // cycles: [8]
            }
            0x23 => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA E");
                anyhow::bail!("opcode SLA E not implemented")
                // cycles: [8]
            }
            0x24 => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA H");
                anyhow::bail!("opcode SLA H not implemented")
                // cycles: [8]
            }
            0x25 => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA L");
                anyhow::bail!("opcode SLA L not implemented")
                // cycles: [8]
            }
            0x26 => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA HL");
                anyhow::bail!("opcode SLA HL not implemented")
                // cycles: [16]
            }
            0x27 => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SLA A");
                anyhow::bail!("opcode SLA A not implemented")
                // cycles: [8]
            }
            0x28 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA B");
                anyhow::bail!("opcode SRA B not implemented")
                // cycles: [8]
            }
            0x29 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA C");
                anyhow::bail!("opcode SRA C not implemented")
                // cycles: [8]
            }
            0x2A => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA D");
                anyhow::bail!("opcode SRA D not implemented")
                // cycles: [8]
            }
            0x2B => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA E");
                anyhow::bail!("opcode SRA E not implemented")
                // cycles: [8]
            }
            0x2C => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA H");
                anyhow::bail!("opcode SRA H not implemented")
                // cycles: [8]
            }
            0x2D => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA L");
                anyhow::bail!("opcode SRA L not implemented")
                // cycles: [8]
            }
            0x2E => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA HL");
                anyhow::bail!("opcode SRA HL not implemented")
                // cycles: [16]
            }
            0x2F => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRA A");
                anyhow::bail!("opcode SRA A not implemented")
                // cycles: [8]
            }
            0x30 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP B");
                anyhow::bail!("opcode SWAP B not implemented")
                // cycles: [8]
            }
            0x31 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP C");
                anyhow::bail!("opcode SWAP C not implemented")
                // cycles: [8]
            }
            0x32 => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP D");
                anyhow::bail!("opcode SWAP D not implemented")
                // cycles: [8]
            }
            0x33 => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP E");
                anyhow::bail!("opcode SWAP E not implemented")
                // cycles: [8]
            }
            0x34 => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP H");
                anyhow::bail!("opcode SWAP H not implemented")
                // cycles: [8]
            }
            0x35 => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP L");
                anyhow::bail!("opcode SWAP L not implemented")
                // cycles: [8]
            }
            0x36 => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP HL");
                anyhow::bail!("opcode SWAP HL not implemented")
                // cycles: [16]
            }
            0x37 => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': '0'}
                tracing::trace!("SWAP A");
                anyhow::bail!("opcode SWAP A not implemented")
                // cycles: [8]
            }
            0x38 => {
                // [{'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL B");
                anyhow::bail!("opcode SRL B not implemented")
                // cycles: [8]
            }
            0x39 => {
                // [{'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL C");
                anyhow::bail!("opcode SRL C not implemented")
                // cycles: [8]
            }
            0x3A => {
                // [{'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL D");
                anyhow::bail!("opcode SRL D not implemented")
                // cycles: [8]
            }
            0x3B => {
                // [{'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL E");
                anyhow::bail!("opcode SRL E not implemented")
                // cycles: [8]
            }
            0x3C => {
                // [{'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL H");
                anyhow::bail!("opcode SRL H not implemented")
                // cycles: [8]
            }
            0x3D => {
                // [{'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL L");
                anyhow::bail!("opcode SRL L not implemented")
                // cycles: [8]
            }
            0x3E => {
                // [{'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL HL");
                anyhow::bail!("opcode SRL HL not implemented")
                // cycles: [16]
            }
            0x3F => {
                // [{'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '0', 'C': 'C'}
                tracing::trace!("SRL A");
                anyhow::bail!("opcode SRL A not implemented")
                // cycles: [8]
            }
            0x40 => {
                // [{'name': '0', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, B");
                anyhow::bail!("opcode BIT 0, B not implemented")
                // cycles: [8]
            }
            0x41 => {
                // [{'name': '0', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, C");
                anyhow::bail!("opcode BIT 0, C not implemented")
                // cycles: [8]
            }
            0x42 => {
                // [{'name': '0', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, D");
                anyhow::bail!("opcode BIT 0, D not implemented")
                // cycles: [8]
            }
            0x43 => {
                // [{'name': '0', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, E");
                anyhow::bail!("opcode BIT 0, E not implemented")
                // cycles: [8]
            }
            0x44 => {
                // [{'name': '0', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, H");
                anyhow::bail!("opcode BIT 0, H not implemented")
                // cycles: [8]
            }
            0x45 => {
                // [{'name': '0', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, L");
                anyhow::bail!("opcode BIT 0, L not implemented")
                // cycles: [8]
            }
            0x46 => {
                // [{'name': '0', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, HL");
                anyhow::bail!("opcode BIT 0, HL not implemented")
                // cycles: [12]
            }
            0x47 => {
                // [{'name': '0', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 0, A");
                anyhow::bail!("opcode BIT 0, A not implemented")
                // cycles: [8]
            }
            0x48 => {
                // [{'name': '1', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, B");
                anyhow::bail!("opcode BIT 1, B not implemented")
                // cycles: [8]
            }
            0x49 => {
                // [{'name': '1', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, C");
                anyhow::bail!("opcode BIT 1, C not implemented")
                // cycles: [8]
            }
            0x4A => {
                // [{'name': '1', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, D");
                anyhow::bail!("opcode BIT 1, D not implemented")
                // cycles: [8]
            }
            0x4B => {
                // [{'name': '1', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, E");
                anyhow::bail!("opcode BIT 1, E not implemented")
                // cycles: [8]
            }
            0x4C => {
                // [{'name': '1', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, H");
                anyhow::bail!("opcode BIT 1, H not implemented")
                // cycles: [8]
            }
            0x4D => {
                // [{'name': '1', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, L");
                anyhow::bail!("opcode BIT 1, L not implemented")
                // cycles: [8]
            }
            0x4E => {
                // [{'name': '1', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, HL");
                anyhow::bail!("opcode BIT 1, HL not implemented")
                // cycles: [12]
            }
            0x4F => {
                // [{'name': '1', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 1, A");
                anyhow::bail!("opcode BIT 1, A not implemented")
                // cycles: [8]
            }
            0x50 => {
                // [{'name': '2', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, B");
                anyhow::bail!("opcode BIT 2, B not implemented")
                // cycles: [8]
            }
            0x51 => {
                // [{'name': '2', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, C");
                anyhow::bail!("opcode BIT 2, C not implemented")
                // cycles: [8]
            }
            0x52 => {
                // [{'name': '2', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, D");
                anyhow::bail!("opcode BIT 2, D not implemented")
                // cycles: [8]
            }
            0x53 => {
                // [{'name': '2', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, E");
                anyhow::bail!("opcode BIT 2, E not implemented")
                // cycles: [8]
            }
            0x54 => {
                // [{'name': '2', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, H");
                anyhow::bail!("opcode BIT 2, H not implemented")
                // cycles: [8]
            }
            0x55 => {
                // [{'name': '2', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, L");
                anyhow::bail!("opcode BIT 2, L not implemented")
                // cycles: [8]
            }
            0x56 => {
                // [{'name': '2', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, HL");
                anyhow::bail!("opcode BIT 2, HL not implemented")
                // cycles: [12]
            }
            0x57 => {
                // [{'name': '2', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 2, A");
                anyhow::bail!("opcode BIT 2, A not implemented")
                // cycles: [8]
            }
            0x58 => {
                // [{'name': '3', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, B");
                anyhow::bail!("opcode BIT 3, B not implemented")
                // cycles: [8]
            }
            0x59 => {
                // [{'name': '3', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, C");
                anyhow::bail!("opcode BIT 3, C not implemented")
                // cycles: [8]
            }
            0x5A => {
                // [{'name': '3', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, D");
                anyhow::bail!("opcode BIT 3, D not implemented")
                // cycles: [8]
            }
            0x5B => {
                // [{'name': '3', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, E");
                anyhow::bail!("opcode BIT 3, E not implemented")
                // cycles: [8]
            }
            0x5C => {
                // [{'name': '3', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, H");
                anyhow::bail!("opcode BIT 3, H not implemented")
                // cycles: [8]
            }
            0x5D => {
                // [{'name': '3', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, L");
                anyhow::bail!("opcode BIT 3, L not implemented")
                // cycles: [8]
            }
            0x5E => {
                // [{'name': '3', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, HL");
                anyhow::bail!("opcode BIT 3, HL not implemented")
                // cycles: [12]
            }
            0x5F => {
                // [{'name': '3', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 3, A");
                anyhow::bail!("opcode BIT 3, A not implemented")
                // cycles: [8]
            }
            0x60 => {
                // [{'name': '4', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, B");
                anyhow::bail!("opcode BIT 4, B not implemented")
                // cycles: [8]
            }
            0x61 => {
                // [{'name': '4', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, C");
                anyhow::bail!("opcode BIT 4, C not implemented")
                // cycles: [8]
            }
            0x62 => {
                // [{'name': '4', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, D");
                anyhow::bail!("opcode BIT 4, D not implemented")
                // cycles: [8]
            }
            0x63 => {
                // [{'name': '4', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, E");
                anyhow::bail!("opcode BIT 4, E not implemented")
                // cycles: [8]
            }
            0x64 => {
                // [{'name': '4', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, H");
                anyhow::bail!("opcode BIT 4, H not implemented")
                // cycles: [8]
            }
            0x65 => {
                // [{'name': '4', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, L");
                anyhow::bail!("opcode BIT 4, L not implemented")
                // cycles: [8]
            }
            0x66 => {
                // [{'name': '4', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, HL");
                anyhow::bail!("opcode BIT 4, HL not implemented")
                // cycles: [12]
            }
            0x67 => {
                // [{'name': '4', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 4, A");
                anyhow::bail!("opcode BIT 4, A not implemented")
                // cycles: [8]
            }
            0x68 => {
                // [{'name': '5', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, B");
                anyhow::bail!("opcode BIT 5, B not implemented")
                // cycles: [8]
            }
            0x69 => {
                // [{'name': '5', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, C");
                anyhow::bail!("opcode BIT 5, C not implemented")
                // cycles: [8]
            }
            0x6A => {
                // [{'name': '5', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, D");
                anyhow::bail!("opcode BIT 5, D not implemented")
                // cycles: [8]
            }
            0x6B => {
                // [{'name': '5', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, E");
                anyhow::bail!("opcode BIT 5, E not implemented")
                // cycles: [8]
            }
            0x6C => {
                // [{'name': '5', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, H");
                anyhow::bail!("opcode BIT 5, H not implemented")
                // cycles: [8]
            }
            0x6D => {
                // [{'name': '5', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, L");
                anyhow::bail!("opcode BIT 5, L not implemented")
                // cycles: [8]
            }
            0x6E => {
                // [{'name': '5', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, HL");
                anyhow::bail!("opcode BIT 5, HL not implemented")
                // cycles: [12]
            }
            0x6F => {
                // [{'name': '5', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 5, A");
                anyhow::bail!("opcode BIT 5, A not implemented")
                // cycles: [8]
            }
            0x70 => {
                // [{'name': '6', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, B");
                anyhow::bail!("opcode BIT 6, B not implemented")
                // cycles: [8]
            }
            0x71 => {
                // [{'name': '6', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, C");
                anyhow::bail!("opcode BIT 6, C not implemented")
                // cycles: [8]
            }
            0x72 => {
                // [{'name': '6', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, D");
                anyhow::bail!("opcode BIT 6, D not implemented")
                // cycles: [8]
            }
            0x73 => {
                // [{'name': '6', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, E");
                anyhow::bail!("opcode BIT 6, E not implemented")
                // cycles: [8]
            }
            0x74 => {
                // [{'name': '6', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, H");
                anyhow::bail!("opcode BIT 6, H not implemented")
                // cycles: [8]
            }
            0x75 => {
                // [{'name': '6', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, L");
                anyhow::bail!("opcode BIT 6, L not implemented")
                // cycles: [8]
            }
            0x76 => {
                // [{'name': '6', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, HL");
                anyhow::bail!("opcode BIT 6, HL not implemented")
                // cycles: [12]
            }
            0x77 => {
                // [{'name': '6', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 6, A");
                anyhow::bail!("opcode BIT 6, A not implemented")
                // cycles: [8]
            }
            0x78 => {
                // [{'name': '7', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, B");
                anyhow::bail!("opcode BIT 7, B not implemented")
                // cycles: [8]
            }
            0x79 => {
                // [{'name': '7', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, C");
                anyhow::bail!("opcode BIT 7, C not implemented")
                // cycles: [8]
            }
            0x7A => {
                // [{'name': '7', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, D");
                anyhow::bail!("opcode BIT 7, D not implemented")
                // cycles: [8]
            }
            0x7B => {
                // [{'name': '7', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, E");
                anyhow::bail!("opcode BIT 7, E not implemented")
                // cycles: [8]
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
            0x7D => {
                // [{'name': '7', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, L");
                anyhow::bail!("opcode BIT 7, L not implemented")
                // cycles: [8]
            }
            0x7E => {
                // [{'name': '7', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, HL");
                anyhow::bail!("opcode BIT 7, HL not implemented")
                // cycles: [12]
            }
            0x7F => {
                // [{'name': '7', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': 'Z', 'N': '0', 'H': '1', 'C': '-'}
                tracing::trace!("BIT 7, A");
                anyhow::bail!("opcode BIT 7, A not implemented")
                // cycles: [8]
            }
            0x80 => {
                // [{'name': '0', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, B");
                anyhow::bail!("opcode RES 0, B not implemented")
                // cycles: [8]
            }
            0x81 => {
                // [{'name': '0', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, C");
                anyhow::bail!("opcode RES 0, C not implemented")
                // cycles: [8]
            }
            0x82 => {
                // [{'name': '0', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, D");
                anyhow::bail!("opcode RES 0, D not implemented")
                // cycles: [8]
            }
            0x83 => {
                // [{'name': '0', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, E");
                anyhow::bail!("opcode RES 0, E not implemented")
                // cycles: [8]
            }
            0x84 => {
                // [{'name': '0', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, H");
                anyhow::bail!("opcode RES 0, H not implemented")
                // cycles: [8]
            }
            0x85 => {
                // [{'name': '0', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, L");
                anyhow::bail!("opcode RES 0, L not implemented")
                // cycles: [8]
            }
            0x86 => {
                // [{'name': '0', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, HL");
                anyhow::bail!("opcode RES 0, HL not implemented")
                // cycles: [16]
            }
            0x87 => {
                // [{'name': '0', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 0, A");
                anyhow::bail!("opcode RES 0, A not implemented")
                // cycles: [8]
            }
            0x88 => {
                // [{'name': '1', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, B");
                anyhow::bail!("opcode RES 1, B not implemented")
                // cycles: [8]
            }
            0x89 => {
                // [{'name': '1', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, C");
                anyhow::bail!("opcode RES 1, C not implemented")
                // cycles: [8]
            }
            0x8A => {
                // [{'name': '1', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, D");
                anyhow::bail!("opcode RES 1, D not implemented")
                // cycles: [8]
            }
            0x8B => {
                // [{'name': '1', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, E");
                anyhow::bail!("opcode RES 1, E not implemented")
                // cycles: [8]
            }
            0x8C => {
                // [{'name': '1', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, H");
                anyhow::bail!("opcode RES 1, H not implemented")
                // cycles: [8]
            }
            0x8D => {
                // [{'name': '1', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, L");
                anyhow::bail!("opcode RES 1, L not implemented")
                // cycles: [8]
            }
            0x8E => {
                // [{'name': '1', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, HL");
                anyhow::bail!("opcode RES 1, HL not implemented")
                // cycles: [16]
            }
            0x8F => {
                // [{'name': '1', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 1, A");
                anyhow::bail!("opcode RES 1, A not implemented")
                // cycles: [8]
            }
            0x90 => {
                // [{'name': '2', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, B");
                anyhow::bail!("opcode RES 2, B not implemented")
                // cycles: [8]
            }
            0x91 => {
                // [{'name': '2', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, C");
                anyhow::bail!("opcode RES 2, C not implemented")
                // cycles: [8]
            }
            0x92 => {
                // [{'name': '2', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, D");
                anyhow::bail!("opcode RES 2, D not implemented")
                // cycles: [8]
            }
            0x93 => {
                // [{'name': '2', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, E");
                anyhow::bail!("opcode RES 2, E not implemented")
                // cycles: [8]
            }
            0x94 => {
                // [{'name': '2', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, H");
                anyhow::bail!("opcode RES 2, H not implemented")
                // cycles: [8]
            }
            0x95 => {
                // [{'name': '2', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, L");
                anyhow::bail!("opcode RES 2, L not implemented")
                // cycles: [8]
            }
            0x96 => {
                // [{'name': '2', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, HL");
                anyhow::bail!("opcode RES 2, HL not implemented")
                // cycles: [16]
            }
            0x97 => {
                // [{'name': '2', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 2, A");
                anyhow::bail!("opcode RES 2, A not implemented")
                // cycles: [8]
            }
            0x98 => {
                // [{'name': '3', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, B");
                anyhow::bail!("opcode RES 3, B not implemented")
                // cycles: [8]
            }
            0x99 => {
                // [{'name': '3', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, C");
                anyhow::bail!("opcode RES 3, C not implemented")
                // cycles: [8]
            }
            0x9A => {
                // [{'name': '3', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, D");
                anyhow::bail!("opcode RES 3, D not implemented")
                // cycles: [8]
            }
            0x9B => {
                // [{'name': '3', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, E");
                anyhow::bail!("opcode RES 3, E not implemented")
                // cycles: [8]
            }
            0x9C => {
                // [{'name': '3', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, H");
                anyhow::bail!("opcode RES 3, H not implemented")
                // cycles: [8]
            }
            0x9D => {
                // [{'name': '3', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, L");
                anyhow::bail!("opcode RES 3, L not implemented")
                // cycles: [8]
            }
            0x9E => {
                // [{'name': '3', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, HL");
                anyhow::bail!("opcode RES 3, HL not implemented")
                // cycles: [16]
            }
            0x9F => {
                // [{'name': '3', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 3, A");
                anyhow::bail!("opcode RES 3, A not implemented")
                // cycles: [8]
            }
            0xA0 => {
                // [{'name': '4', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, B");
                anyhow::bail!("opcode RES 4, B not implemented")
                // cycles: [8]
            }
            0xA1 => {
                // [{'name': '4', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, C");
                anyhow::bail!("opcode RES 4, C not implemented")
                // cycles: [8]
            }
            0xA2 => {
                // [{'name': '4', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, D");
                anyhow::bail!("opcode RES 4, D not implemented")
                // cycles: [8]
            }
            0xA3 => {
                // [{'name': '4', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, E");
                anyhow::bail!("opcode RES 4, E not implemented")
                // cycles: [8]
            }
            0xA4 => {
                // [{'name': '4', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, H");
                anyhow::bail!("opcode RES 4, H not implemented")
                // cycles: [8]
            }
            0xA5 => {
                // [{'name': '4', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, L");
                anyhow::bail!("opcode RES 4, L not implemented")
                // cycles: [8]
            }
            0xA6 => {
                // [{'name': '4', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, HL");
                anyhow::bail!("opcode RES 4, HL not implemented")
                // cycles: [16]
            }
            0xA7 => {
                // [{'name': '4', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 4, A");
                anyhow::bail!("opcode RES 4, A not implemented")
                // cycles: [8]
            }
            0xA8 => {
                // [{'name': '5', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, B");
                anyhow::bail!("opcode RES 5, B not implemented")
                // cycles: [8]
            }
            0xA9 => {
                // [{'name': '5', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, C");
                anyhow::bail!("opcode RES 5, C not implemented")
                // cycles: [8]
            }
            0xAA => {
                // [{'name': '5', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, D");
                anyhow::bail!("opcode RES 5, D not implemented")
                // cycles: [8]
            }
            0xAB => {
                // [{'name': '5', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, E");
                anyhow::bail!("opcode RES 5, E not implemented")
                // cycles: [8]
            }
            0xAC => {
                // [{'name': '5', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, H");
                anyhow::bail!("opcode RES 5, H not implemented")
                // cycles: [8]
            }
            0xAD => {
                // [{'name': '5', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, L");
                anyhow::bail!("opcode RES 5, L not implemented")
                // cycles: [8]
            }
            0xAE => {
                // [{'name': '5', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, HL");
                anyhow::bail!("opcode RES 5, HL not implemented")
                // cycles: [16]
            }
            0xAF => {
                // [{'name': '5', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 5, A");
                anyhow::bail!("opcode RES 5, A not implemented")
                // cycles: [8]
            }
            0xB0 => {
                // [{'name': '6', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, B");
                anyhow::bail!("opcode RES 6, B not implemented")
                // cycles: [8]
            }
            0xB1 => {
                // [{'name': '6', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, C");
                anyhow::bail!("opcode RES 6, C not implemented")
                // cycles: [8]
            }
            0xB2 => {
                // [{'name': '6', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, D");
                anyhow::bail!("opcode RES 6, D not implemented")
                // cycles: [8]
            }
            0xB3 => {
                // [{'name': '6', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, E");
                anyhow::bail!("opcode RES 6, E not implemented")
                // cycles: [8]
            }
            0xB4 => {
                // [{'name': '6', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, H");
                anyhow::bail!("opcode RES 6, H not implemented")
                // cycles: [8]
            }
            0xB5 => {
                // [{'name': '6', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, L");
                anyhow::bail!("opcode RES 6, L not implemented")
                // cycles: [8]
            }
            0xB6 => {
                // [{'name': '6', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, HL");
                anyhow::bail!("opcode RES 6, HL not implemented")
                // cycles: [16]
            }
            0xB7 => {
                // [{'name': '6', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 6, A");
                anyhow::bail!("opcode RES 6, A not implemented")
                // cycles: [8]
            }
            0xB8 => {
                // [{'name': '7', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, B");
                anyhow::bail!("opcode RES 7, B not implemented")
                // cycles: [8]
            }
            0xB9 => {
                // [{'name': '7', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, C");
                anyhow::bail!("opcode RES 7, C not implemented")
                // cycles: [8]
            }
            0xBA => {
                // [{'name': '7', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, D");
                anyhow::bail!("opcode RES 7, D not implemented")
                // cycles: [8]
            }
            0xBB => {
                // [{'name': '7', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, E");
                anyhow::bail!("opcode RES 7, E not implemented")
                // cycles: [8]
            }
            0xBC => {
                // [{'name': '7', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, H");
                anyhow::bail!("opcode RES 7, H not implemented")
                // cycles: [8]
            }
            0xBD => {
                // [{'name': '7', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, L");
                anyhow::bail!("opcode RES 7, L not implemented")
                // cycles: [8]
            }
            0xBE => {
                // [{'name': '7', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, HL");
                anyhow::bail!("opcode RES 7, HL not implemented")
                // cycles: [16]
            }
            0xBF => {
                // [{'name': '7', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("RES 7, A");
                anyhow::bail!("opcode RES 7, A not implemented")
                // cycles: [8]
            }
            0xC0 => {
                // [{'name': '0', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, B");
                anyhow::bail!("opcode SET 0, B not implemented")
                // cycles: [8]
            }
            0xC1 => {
                // [{'name': '0', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, C");
                anyhow::bail!("opcode SET 0, C not implemented")
                // cycles: [8]
            }
            0xC2 => {
                // [{'name': '0', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, D");
                anyhow::bail!("opcode SET 0, D not implemented")
                // cycles: [8]
            }
            0xC3 => {
                // [{'name': '0', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, E");
                anyhow::bail!("opcode SET 0, E not implemented")
                // cycles: [8]
            }
            0xC4 => {
                // [{'name': '0', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, H");
                anyhow::bail!("opcode SET 0, H not implemented")
                // cycles: [8]
            }
            0xC5 => {
                // [{'name': '0', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, L");
                anyhow::bail!("opcode SET 0, L not implemented")
                // cycles: [8]
            }
            0xC6 => {
                // [{'name': '0', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, HL");
                anyhow::bail!("opcode SET 0, HL not implemented")
                // cycles: [16]
            }
            0xC7 => {
                // [{'name': '0', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 0, A");
                anyhow::bail!("opcode SET 0, A not implemented")
                // cycles: [8]
            }
            0xC8 => {
                // [{'name': '1', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, B");
                anyhow::bail!("opcode SET 1, B not implemented")
                // cycles: [8]
            }
            0xC9 => {
                // [{'name': '1', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, C");
                anyhow::bail!("opcode SET 1, C not implemented")
                // cycles: [8]
            }
            0xCA => {
                // [{'name': '1', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, D");
                anyhow::bail!("opcode SET 1, D not implemented")
                // cycles: [8]
            }
            0xCB => {
                // [{'name': '1', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, E");
                anyhow::bail!("opcode SET 1, E not implemented")
                // cycles: [8]
            }
            0xCC => {
                // [{'name': '1', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, H");
                anyhow::bail!("opcode SET 1, H not implemented")
                // cycles: [8]
            }
            0xCD => {
                // [{'name': '1', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, L");
                anyhow::bail!("opcode SET 1, L not implemented")
                // cycles: [8]
            }
            0xCE => {
                // [{'name': '1', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, HL");
                anyhow::bail!("opcode SET 1, HL not implemented")
                // cycles: [16]
            }
            0xCF => {
                // [{'name': '1', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 1, A");
                anyhow::bail!("opcode SET 1, A not implemented")
                // cycles: [8]
            }
            0xD0 => {
                // [{'name': '2', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, B");
                anyhow::bail!("opcode SET 2, B not implemented")
                // cycles: [8]
            }
            0xD1 => {
                // [{'name': '2', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, C");
                anyhow::bail!("opcode SET 2, C not implemented")
                // cycles: [8]
            }
            0xD2 => {
                // [{'name': '2', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, D");
                anyhow::bail!("opcode SET 2, D not implemented")
                // cycles: [8]
            }
            0xD3 => {
                // [{'name': '2', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, E");
                anyhow::bail!("opcode SET 2, E not implemented")
                // cycles: [8]
            }
            0xD4 => {
                // [{'name': '2', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, H");
                anyhow::bail!("opcode SET 2, H not implemented")
                // cycles: [8]
            }
            0xD5 => {
                // [{'name': '2', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, L");
                anyhow::bail!("opcode SET 2, L not implemented")
                // cycles: [8]
            }
            0xD6 => {
                // [{'name': '2', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, HL");
                anyhow::bail!("opcode SET 2, HL not implemented")
                // cycles: [16]
            }
            0xD7 => {
                // [{'name': '2', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 2, A");
                anyhow::bail!("opcode SET 2, A not implemented")
                // cycles: [8]
            }
            0xD8 => {
                // [{'name': '3', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, B");
                anyhow::bail!("opcode SET 3, B not implemented")
                // cycles: [8]
            }
            0xD9 => {
                // [{'name': '3', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, C");
                anyhow::bail!("opcode SET 3, C not implemented")
                // cycles: [8]
            }
            0xDA => {
                // [{'name': '3', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, D");
                anyhow::bail!("opcode SET 3, D not implemented")
                // cycles: [8]
            }
            0xDB => {
                // [{'name': '3', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, E");
                anyhow::bail!("opcode SET 3, E not implemented")
                // cycles: [8]
            }
            0xDC => {
                // [{'name': '3', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, H");
                anyhow::bail!("opcode SET 3, H not implemented")
                // cycles: [8]
            }
            0xDD => {
                // [{'name': '3', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, L");
                anyhow::bail!("opcode SET 3, L not implemented")
                // cycles: [8]
            }
            0xDE => {
                // [{'name': '3', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, HL");
                anyhow::bail!("opcode SET 3, HL not implemented")
                // cycles: [16]
            }
            0xDF => {
                // [{'name': '3', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 3, A");
                anyhow::bail!("opcode SET 3, A not implemented")
                // cycles: [8]
            }
            0xE0 => {
                // [{'name': '4', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, B");
                anyhow::bail!("opcode SET 4, B not implemented")
                // cycles: [8]
            }
            0xE1 => {
                // [{'name': '4', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, C");
                anyhow::bail!("opcode SET 4, C not implemented")
                // cycles: [8]
            }
            0xE2 => {
                // [{'name': '4', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, D");
                anyhow::bail!("opcode SET 4, D not implemented")
                // cycles: [8]
            }
            0xE3 => {
                // [{'name': '4', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, E");
                anyhow::bail!("opcode SET 4, E not implemented")
                // cycles: [8]
            }
            0xE4 => {
                // [{'name': '4', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, H");
                anyhow::bail!("opcode SET 4, H not implemented")
                // cycles: [8]
            }
            0xE5 => {
                // [{'name': '4', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, L");
                anyhow::bail!("opcode SET 4, L not implemented")
                // cycles: [8]
            }
            0xE6 => {
                // [{'name': '4', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, HL");
                anyhow::bail!("opcode SET 4, HL not implemented")
                // cycles: [16]
            }
            0xE7 => {
                // [{'name': '4', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 4, A");
                anyhow::bail!("opcode SET 4, A not implemented")
                // cycles: [8]
            }
            0xE8 => {
                // [{'name': '5', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, B");
                anyhow::bail!("opcode SET 5, B not implemented")
                // cycles: [8]
            }
            0xE9 => {
                // [{'name': '5', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, C");
                anyhow::bail!("opcode SET 5, C not implemented")
                // cycles: [8]
            }
            0xEA => {
                // [{'name': '5', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, D");
                anyhow::bail!("opcode SET 5, D not implemented")
                // cycles: [8]
            }
            0xEB => {
                // [{'name': '5', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, E");
                anyhow::bail!("opcode SET 5, E not implemented")
                // cycles: [8]
            }
            0xEC => {
                // [{'name': '5', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, H");
                anyhow::bail!("opcode SET 5, H not implemented")
                // cycles: [8]
            }
            0xED => {
                // [{'name': '5', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, L");
                anyhow::bail!("opcode SET 5, L not implemented")
                // cycles: [8]
            }
            0xEE => {
                // [{'name': '5', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, HL");
                anyhow::bail!("opcode SET 5, HL not implemented")
                // cycles: [16]
            }
            0xEF => {
                // [{'name': '5', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 5, A");
                anyhow::bail!("opcode SET 5, A not implemented")
                // cycles: [8]
            }
            0xF0 => {
                // [{'name': '6', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, B");
                anyhow::bail!("opcode SET 6, B not implemented")
                // cycles: [8]
            }
            0xF1 => {
                // [{'name': '6', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, C");
                anyhow::bail!("opcode SET 6, C not implemented")
                // cycles: [8]
            }
            0xF2 => {
                // [{'name': '6', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, D");
                anyhow::bail!("opcode SET 6, D not implemented")
                // cycles: [8]
            }
            0xF3 => {
                // [{'name': '6', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, E");
                anyhow::bail!("opcode SET 6, E not implemented")
                // cycles: [8]
            }
            0xF4 => {
                // [{'name': '6', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, H");
                anyhow::bail!("opcode SET 6, H not implemented")
                // cycles: [8]
            }
            0xF5 => {
                // [{'name': '6', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, L");
                anyhow::bail!("opcode SET 6, L not implemented")
                // cycles: [8]
            }
            0xF6 => {
                // [{'name': '6', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, HL");
                anyhow::bail!("opcode SET 6, HL not implemented")
                // cycles: [16]
            }
            0xF7 => {
                // [{'name': '6', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 6, A");
                anyhow::bail!("opcode SET 6, A not implemented")
                // cycles: [8]
            }
            0xF8 => {
                // [{'name': '7', 'immediate': True}, {'name': 'B', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, B");
                anyhow::bail!("opcode SET 7, B not implemented")
                // cycles: [8]
            }
            0xF9 => {
                // [{'name': '7', 'immediate': True}, {'name': 'C', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, C");
                anyhow::bail!("opcode SET 7, C not implemented")
                // cycles: [8]
            }
            0xFA => {
                // [{'name': '7', 'immediate': True}, {'name': 'D', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, D");
                anyhow::bail!("opcode SET 7, D not implemented")
                // cycles: [8]
            }
            0xFB => {
                // [{'name': '7', 'immediate': True}, {'name': 'E', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, E");
                anyhow::bail!("opcode SET 7, E not implemented")
                // cycles: [8]
            }
            0xFC => {
                // [{'name': '7', 'immediate': True}, {'name': 'H', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, H");
                anyhow::bail!("opcode SET 7, H not implemented")
                // cycles: [8]
            }
            0xFD => {
                // [{'name': '7', 'immediate': True}, {'name': 'L', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, L");
                anyhow::bail!("opcode SET 7, L not implemented")
                // cycles: [8]
            }
            0xFE => {
                // [{'name': '7', 'immediate': True}, {'name': 'HL', 'immediate': False}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, HL");
                anyhow::bail!("opcode SET 7, HL not implemented")
                // cycles: [16]
            }
            0xFF => {
                // [{'name': '7', 'immediate': True}, {'name': 'A', 'immediate': True}]
                // {'Z': '-', 'N': '-', 'H': '-', 'C': '-'}
                tracing::trace!("SET 7, A");
                anyhow::bail!("opcode SET 7, A not implemented")
                // cycles: [8]
            } // codegen-prefix-end
              // _ => anyhow::bail!("Not implemented prefix_opcode={:08b}", opcode),
        };

        Ok(n_cycles)
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
