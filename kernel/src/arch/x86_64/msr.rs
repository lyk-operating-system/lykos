use core::arch::asm;

#[repr(u32)]
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Msr {
    ApicBase = 0x1B,
    Pat = 0x277,
    Efer = 0xC0000080,
    Star = 0xC0000081,
    Lstar = 0xC0000082,
    Cstar = 0xC0000083,
    Sfmask = 0xC0000084,
    FsBase = 0xC0000100,
    GsBase = 0xC0000101,
    KernelGsBase = 0xC0000102,
}

#[inline(always)]
pub unsafe fn read(msr: Msr) -> u64 {
    let low: u32;
    let high: u32;

    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr as u32,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }

    ((high as u64) << 32) | (low as u64)
}

#[inline(always)]
pub unsafe fn write(msr: Msr, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr as u32,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack, preserves_flags),
        );
    }
}
