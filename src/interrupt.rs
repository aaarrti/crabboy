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
