#![no_std]
#![no_main]
#![feature(offset_of_enum)]

mod arch;
mod boot;
mod logger;
mod memory;
mod panic;
mod print;
mod sync;

#[unsafe(no_mangle)]
unsafe extern "C" fn kernel_main() -> ! {
    assert!(boot::BASE_REVISION.is_supported());

    memory::buddy::init();
    memory::vm::init();

    arch::lcpu::halt_forever();
}
