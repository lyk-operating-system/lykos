use core::fmt::{self, Write};

use crate::logger::Logger;

pub fn _print(args: fmt::Arguments) {
    let mut logger = Logger;
    let _ = logger.write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::print::_print(core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::print!("\n");
    }};

    ($($arg:tt)*) => {{
        $crate::print!("{}\n", core::format_args!($($arg)*));
    }};
}
