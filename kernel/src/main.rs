#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

mod allocator;
mod framebuffer;
mod gdt;
mod interrupts;
mod keyboard;
mod memory;
mod serial;
mod shell;

use bootloader_api::{BootInfo, entry_point};
use x86_64::VirtAddr;

#[allow(deprecated)]
const CONFIG: bootloader_api::BootloaderConfig = {
    let mut config = bootloader_api::BootloaderConfig::new_default();
    config.frame_buffer.minimum_framebuffer_width = Some(1024);
    config.frame_buffer.minimum_framebuffer_height = Some(768);
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    serial::init();

    // フレームバッファコンソールを初期化（以降の println! が画面にも出力される）
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let width = info.width.min(640);
        let height = info.height.min(400);
        let window = framebuffer::Window {
            x: (info.width - width) / 2,
            y: (info.height - height) / 2,
            width,
            height,
        };
        let buf = fb.buffer_mut();
        framebuffer::init(buf, info, window);
    }

    gdt::init();
    interrupts::init();
    interrupts::init_pics();
    x86_64::instructions::interrupts::enable();

    #[cfg(test)]
    test_main();

    println!("\n=== Memory Map ===");

    let mut total_usable: u64 = 0;
    for region in boot_info.memory_regions.iter() {
        let size = region.end - region.start;
        println!(
            "{:#018x} {:>8} KiB {:?}",
            region.start,
            size / 1024,
            region.kind,
        );
        if matches!(region.kind, bootloader_api::info::MemoryRegionKind::Usable) {
            total_usable += size;
        }
    }

    println!(
        "Total usable: {} KiB ({} MiB)",
        total_usable / 1024,
        total_usable / (1024 * 1024),
    );

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

    shell::run();
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
