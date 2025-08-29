/// Checks if the nth bit (0-based) is set
#[inline(always)]
pub fn is_nth_bit_set(x: u8, n: usize) -> bool {
    (x & (1 << n)) != 0
}

/// Sets the nth bit (0-based)
#[inline(always)]
pub fn set_nth_bit(x: u8, n: usize) -> u8 {
    x | (1 << n)
}

/// Clears the nth bit (0-based)
#[inline(always)]
pub fn clear_nth_bit(x: u8, n: usize) -> u8 {
    x & !(1 << n)
}
