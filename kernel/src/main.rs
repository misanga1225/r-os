#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

mod allocator;
mod counter;
mod framebuffer;
mod gdt;
mod interrupts;
mod keyboard;
mod memmap;
mod memory;
mod mouse;
mod serial;
mod shell;
mod task;
mod wm;

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

    // Initialize framebuffer (draws background, stores in FB_STATE)
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let buf = fb.buffer_mut();
        framebuffer::init(buf, info);
    }

    gdt::init();
    interrupts::init();
    interrupts::init_pics();
    mouse::init();
    x86_64::instructions::interrupts::enable();

    #[cfg(test)]
    test_main();

    // Initialize page table, frame allocator, and heap (8 MiB)
    let phys_offset = VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("physical_memory_offset not available"),
    );
    let mut mapper = unsafe { memory::init_page_table(phys_offset) };
    let mut frame_allocator = memory::BootFrameAllocator::new(&boot_info.memory_regions);
    allocator::init(&mut mapper, &mut frame_allocator);

    // ---------- Window Manager ----------
    wm::init();

    // Collect memory regions
    {
        use bootloader_api::info::MemoryRegionKind as BK;
        use framebuffer::{MemRegionInfo, MemRegionKind};

        let mut regions = [MemRegionInfo {
            start: 0,
            end: 0,
            kind: MemRegionKind::Usable,
        }; 32];
        let mut count = 0;

        for region in boot_info.memory_regions.iter() {
            if count >= 30 {
                break;
            }
            let kind = match region.kind {
                BK::Usable => MemRegionKind::Usable,
                BK::Bootloader => MemRegionKind::Bootloader,
                BK::UnknownBios(tag) => framebuffer::bios_e820_to_kind(tag),
                BK::UnknownUefi(_) => MemRegionKind::Reserved,
                _ => MemRegionKind::Reserved,
            };
            regions[count] = MemRegionInfo {
                start: region.start,
                end: region.end,
                kind,
            };
            count += 1;
        }

        // Synthetic: Heap
        if count < 32 {
            regions[count] = MemRegionInfo {
                start: allocator::HEAP_START,
                end: allocator::HEAP_START + allocator::HEAP_SIZE,
                kind: MemRegionKind::Heap,
            };
            count += 1;
        }

        memmap::set_memory_regions(&regions[..count]);
    }

    // Create windows (taskbar is at bottom, 28px)
    // Screen: 1024x768, usable area: 1024x740 (y: 0..740)
    let shell_win = wm::create_window(380, 80, 520, 420, "Shell", 1);
    let memmap_win = wm::create_window(20, 40, 340, 520, "Memory Map", 2);
    let counter_win = wm::create_window(560, 40, 240, 200, "Counter", 3);

    // Init consoles for text-based windows
    wm::init_console(shell_win);
    wm::init_console(counter_win);

    // Initial composite
    wm::composite();

    // ---------- Multitasking ----------
    task::init();

    task::spawn(shell::task_main, Some(shell_win));
    task::spawn(memmap::task_main, Some(memmap_win));
    task::spawn(counter::task_main, Some(counter_win));

    // Start scheduler (never returns)
    task::run();
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
    serial::_print(format_args!("Running {} tests...\n", tests.len()));
    for test in tests {
        test();
    }
    serial::_print(format_args!("All tests passed!\n"));
    exit_qemu(QemuExitCode::Success);
}

#[cfg(test)]
#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial::_print(format_args!("KERNEL PANIC: {info}\n"));
    exit_qemu(QemuExitCode::Failed);
}
