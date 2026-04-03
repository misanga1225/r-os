use spin::Mutex;
use uart_16550::SerialPort;

static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

pub fn init() {
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    *SERIAL.lock() = Some(port);
}

/// シリアルポートに書き込む内部関数。
#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;

    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(serial) = SERIAL.lock().as_mut() {
            serial.write_fmt(args).unwrap();
        }
    });
}

/// シリアル＋フレームバッファの両方に出力する内部関数。マクロから呼び出される。
#[doc(hidden)]
pub fn _print_all(args: core::fmt::Arguments) {
    _print(args);
    crate::framebuffer::_print(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial::_print_all(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! println {
    ()            => { $crate::serial::_print_all(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::serial::_print_all(format_args!("{}\n", format_args!($($arg)*))) };
}
