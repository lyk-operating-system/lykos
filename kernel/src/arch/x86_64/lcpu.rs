use core::arch::asm;

#[inline(always)]
pub fn halt() {
    unsafe {
        asm!("hlt");
    }
}

#[inline(always)]
pub fn halt_forever() -> ! {
    loop {
        halt();
    }
}

#[inline(always)]
pub unsafe fn int_mask() {
    unsafe {
        asm!("cli");
    }
}

#[inline(always)]
pub unsafe fn int_unmask() {
    unsafe {
        asm!("sti");
    }
}
