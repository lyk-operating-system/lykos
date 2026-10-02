use core::fmt::{self, Write};

use crate::{arch::serial, sync::spinlock::Spinlock};

const BUFFER_SIZE: usize = 16 * 1024;

pub struct LoggerBuffer {
    buffer: [u8; BUFFER_SIZE],
    position: usize,
}

pub static LOGGER_BUFFER: Spinlock<LoggerBuffer> = Spinlock::new(LoggerBuffer {
    buffer: [0; BUFFER_SIZE],
    position: 0,
});

pub struct Logger;

impl Write for Logger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut logger_buffer = LOGGER_BUFFER.lock();

        let bytes = s.as_bytes();
        let available = BUFFER_SIZE.saturating_sub(logger_buffer.position);
        let len = core::cmp::min(bytes.len(), available);

        let start = logger_buffer.position;
        let end = start + len;

        logger_buffer.buffer[start..end].copy_from_slice(&bytes[..len]);
        logger_buffer.position = end;

        serial::write_str(s);

        Ok(())
    }
}
