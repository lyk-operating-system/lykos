use crate::{arch::lcpu, println};

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    lcpu::irq_disable();

    println!();
    println!("*** KERNEL PANIC ***");
    if let Some(location) = info.location() {
        println!(
            "at {}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }
    println!("{}", info.message());

    lcpu::halt_forever();
}
