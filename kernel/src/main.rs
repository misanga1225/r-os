#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

mod allocator;
mod framebuffer;
mod memory;
mod serial;

use alloc::{boxed::Box, vec::Vec};
use bootloader_api::{BootInfo, entry_point};
use font8x8::UnicodeFonts;
use x86_64::VirtAddr;

const CONFIG: bootloader_api::BootloaderConfig = {
    let mut config = bootloader_api::BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::init();

    #[cfg(test)]
    test_main();

    const HELLO_AA: &[&str] = &[
        r" _          _ _        ",
        r"| |__   ___| | | ___   ",
        r"| '_ \ / _ \ | |/ _ \  ",
        r"| | | |  __/ | | (_) | ",
        r"|_| |_|\___|_|_|\___/  ",
    ];

    for line in HELLO_AA {
        println!("{}", line);
    }

    println!("\n=== Memory Map ===");
    println!("{:<20} {:<20} {:<12} Kind", "Start", "End", "Size (KiB)");
    println!("{:-<70}", "");

    let mut total_usable: u64 = 0;
    for region in boot_info.memory_regions.iter() {
        let size = region.end - region.start;
        println!(
            "{:#018x}  {:#018x}  {:>10}  {:?}",
            region.start,
            region.end,
            size / 1024,
            region.kind,
        );
        if matches!(region.kind, bootloader_api::info::MemoryRegionKind::Usable) {
            total_usable += size;
        }
    }

    println!("{:-<70}", "");
    println!(
        "Total usable memory: {} KiB ({} MiB)",
        total_usable / 1024,
        total_usable / (1024 * 1024),
    );

    // Framebuffer output
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buf = fb.buffer_mut();

        buf.fill(0);

        for (line_idx, line) in HELLO_AA.iter().enumerate() {
            for (i, ch) in line.chars().enumerate() {
                if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                    for (row, &byte) in glyph.iter().enumerate() {
                        for col in 0..8 {
                            if byte & (1 << col) != 0 {
                                let x = i * 8 + col;
                                let y = line_idx * 8 + row;
                                framebuffer::put_pixel(buf, info, x, y, 0xFF, 0xFF, 0xFF);
                            }
                        }
                    }
                }
            }
        }
        println!(
            "Framebuffer: {}x{}, {:?}",
            info.width, info.height, info.pixel_format
        );
    }

    // Initialize page table, frame allocator, and heap
    let phys_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("physical_memory_offset not available"),
    );
    let mut mapper = unsafe { memory::init_page_table(phys_offset) };
    let mut frame_allocator = memory::BootFrameAllocator::new(&boot_info.memory_regions);
    allocator::init(&mut mapper, &mut frame_allocator);

    // Heap allocation test
    println!("\n=== Heap Allocator Test ===");

    let boxed = Box::new(42);
    println!("Box::new(42) = {}, at {:p}", boxed, boxed);

    let mut vec = Vec::new();
    for i in 0..10 {
        vec.push(i);
    }
    println!("Vec: {:?}", vec);

    let msg = alloc::string::String::from("hello from the heap!");
    println!("String: {}", msg);

    println!("Halting CPU. Close QEMU window to exit.");
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

pub fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests...", tests.len());
    for test in tests {
        test();
    }
    println!("All tests passed!");
    exit_qemu(QemuExitCode::Success);
}

#[cfg(test)]
#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("KERNEL PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}
