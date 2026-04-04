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
mod mouse;
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
        // Shell: 右側パネル
        let shell_window = framebuffer::Window {
            x: 364,
            y: 42,
            width: 646,
            height: 712,
            title_bar_height: framebuffer::DEFAULT_TITLE_BAR_HEIGHT,
        };
        let buf = fb.buffer_mut();
        framebuffer::init(buf, info, shell_window, "Shell");
    }

    // タスクバー描画
    framebuffer::draw_taskbar();

    gdt::init();
    interrupts::init();
    interrupts::init_pics();
    mouse::init();
    x86_64::instructions::interrupts::enable();

    #[cfg(test)]
    test_main();

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

    // メモリ領域を収集してメモリマップパネルに表示
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
                break; // 合成エントリ用に2枠残す
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

        // 合成エントリ: Heap
        if count < 32 {
            regions[count] = MemRegionInfo {
                start: allocator::HEAP_START,
                end: allocator::HEAP_START + allocator::HEAP_SIZE,
                kind: MemRegionKind::Heap,
            };
            count += 1;
        }

        framebuffer::set_memory_regions(&regions[..count]);
    }

    // Memory Map パネル描画（左側）
    let memmap_window = framebuffer::Window {
        x: 14,
        y: 42,
        width: 340,
        height: 712,
        title_bar_height: framebuffer::DEFAULT_TITLE_BAR_HEIGHT,
    };
    framebuffer::draw_memory_map_panel(memmap_window);

    framebuffer::init_cursor();

    println!("Welcome to r-os shell. Type 'help' for available commands.\n");
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
