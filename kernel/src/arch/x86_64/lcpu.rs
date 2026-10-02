use core::arch::asm;

use crate::arch::msr;

#[inline(always)]
pub fn halt() {
    unsafe {
        asm!("hlt", options(nomem, nostack));
    }
}

#[inline(always)]
pub fn halt_forever() -> ! {
    loop {
        halt();
    }
}

#[inline(always)]
pub fn irq_enable() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

#[inline(always)]
pub fn irq_disable() {
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
}

#[inline(always)]
pub fn irq_are_enabled() -> bool {
    let flags: usize;

    unsafe {
        asm!(
            "pushf",
            "pop {}",
            out(reg) flags,
            options(nomem, preserves_flags)
        );
    }

    (flags & (1 << 9)) != 0
}

#[inline(always)]
pub fn irq_save() -> bool {
    let was_enabled = irq_are_enabled();
    irq_disable();
    was_enabled
}

#[inline(always)]
pub fn irq_restore(was_enabled: bool) {
    if was_enabled {
        irq_enable();
    } else {
        irq_disable();
    }
}

#[inline(always)]
pub fn cpu_local_base() -> *mut u8 {
    let ptr: *mut u8;
    unsafe {
        asm!(
            "mov {}, gs:[0]",
            out(reg) ptr,
            options(nostack, preserves_flags, readonly),
        );
    }
    ptr
}

#[inline(always)]
pub fn set_cpu_local_base(base: *mut u8) {
    unsafe { msr::write(msr::Msr::GsBase, base as u64) }
}
