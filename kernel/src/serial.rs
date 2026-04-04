use spin::Mutex;
use uart_16550::SerialPort;

static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

pub fn init() {
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    *SERIAL.lock() = Some(port);
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;

    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(serial) = SERIAL.lock().as_mut() {
            serial.write_fmt(args).unwrap();
        }
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    ()            => { $crate::serial::_print(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::serial::_print(format_args!("{}\n", format_args!($($arg)*))) };
}
