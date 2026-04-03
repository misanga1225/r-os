#![no_std]
#![no_main]

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use bootloader_api::{BootInfo, entry_point};
use core::fmt::Write;
use font8x8::UnicodeFonts;

entry_point!(kernel_main);

fn put_pixel(buf: &mut [u8], info: FrameBufferInfo, x: usize, y: usize, r: u8, g: u8, b: u8) {
    if x >= info.width || y >= info.height {
        return;
    }
    let bpp = info.bytes_per_pixel;
    let offset = (y * info.stride + x) * bpp;
    if offset + bpp > buf.len() {
        return;
    }
    match info.pixel_format {
        PixelFormat::Rgb => {
            buf[offset] = r;
            buf[offset + 1] = g;
            buf[offset + 2] = b;
            if bpp >= 4 {
                buf[offset + 3] = 0xFF;
            }
        }
        PixelFormat::Bgr => {
            buf[offset] = b;
            buf[offset + 1] = g;
            buf[offset + 2] = r;
            if bpp >= 4 {
                buf[offset + 3] = 0xFF;
            }
        }
        PixelFormat::U8 => {
            buf[offset] = ((r as u16 + g as u16 + b as u16) / 3) as u8;
        }
        PixelFormat::Unknown {
            red_position,
            green_position,
            blue_position,
        } => {
            let mut pixel: u32 = 0;
            pixel |= (r as u32) << red_position;
            pixel |= (g as u32) << green_position;
            pixel |= (b as u32) << blue_position;
            let bytes = pixel.to_le_bytes();
            for i in 0..bpp.min(4) {
                buf[offset + i] = bytes[i];
            }
        }
        _ => return,
    }
}

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let mut serial = unsafe { uart_16550::SerialPort::new(0x3F8) };
    serial.init();
    const HELLO_AA: &[&str] = &[
        r" _          _ _        ",
        r"| |__   ___| | | ___   ",
        r"| '_ \ / _ \ | |/ _ \  ",
        r"| | | |  __/ | | (_) | ",
        r"|_| |_|\___|_|_|\___/  ",
    ];

    for line in HELLO_AA {
        writeln!(serial, "{}", line).unwrap();
    }

    writeln!(serial, "\n=== Memory Map ===").unwrap();
    writeln!(
        serial,
        "{:<20} {:<20} {:<12} {}",
        "Start", "End", "Size (KiB)", "Kind"
    )
    .unwrap();
    writeln!(serial, "{:-<70}", "").unwrap();

    let mut total_usable: u64 = 0;
    for region in boot_info.memory_regions.iter() {
        let size = region.end - region.start;
        writeln!(
            serial,
            "{:#018x}  {:#018x}  {:>10}  {:?}",
            region.start,
            region.end,
            size / 1024,
            region.kind,
        )
        .unwrap();
        if matches!(region.kind, bootloader_api::info::MemoryRegionKind::Usable) {
            total_usable += size;
        }
    }

    writeln!(serial, "{:-<70}", "").unwrap();
    writeln!(
        serial,
        "Total usable memory: {} KiB ({} MiB)",
        total_usable / 1024,
        total_usable / (1024 * 1024)
    )
    .unwrap();

    // Framebuffer output
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buf = fb.buffer_mut();

        // Clear screen to black
        buf.fill(0);

        // Draw ASCII art "hello" at top-left
        for (line_idx, line) in HELLO_AA.iter().enumerate() {
            for (i, ch) in line.chars().enumerate() {
                if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                    for (row, &byte) in glyph.iter().enumerate() {
                        for col in 0..8 {
                            if byte & (1 << col) != 0 {
                                let x = i * 8 + col;
                                let y = line_idx * 8 + row;
                                put_pixel(buf, info, x, y, 0xFF, 0xFF, 0xFF);
                            }
                        }
                    }
                }
            }
        }
        writeln!(
            serial,
            "Framebuffer: {}x{}, {:?}",
            info.width, info.height, info.pixel_format
        )
        .unwrap();
    }

    writeln!(serial, "Halting CPU. Close QEMU window to exit.").unwrap();
    loop {
        x86_64::instructions::hlt();
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }

    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut serial = unsafe { uart_16550::SerialPort::new(0x3F8) };
    serial.init();
    let _ = writeln!(serial, "KERNEL PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}
