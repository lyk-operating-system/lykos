use crate::arch::ioport::{inb, outb};

const PORT: u16 = 0x3F8; // COM1

#[inline(always)]
fn tx_empty() -> bool {
    (unsafe { inb(PORT + 5) } & 0x20) != 0
}

pub fn write_str(s: &str) {
    for &b in s.as_bytes() {
        while !tx_empty() {}

        unsafe {
            outb(PORT, b);
        }
    }
}
