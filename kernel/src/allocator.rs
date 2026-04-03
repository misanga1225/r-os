use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

/// カーネルヒープの仮想アドレス開始位置。
///
/// ユーザ空間やカーネルコードと衝突しないカノニカルアドレスを選択する。
pub const HEAP_START: u64 = 0x4444_4444_0000;

/// カーネルヒープのサイズ（100 KiB）。
pub const HEAP_SIZE: u64 = 100 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// ヒープ領域の仮想ページを物理フレームにマップし、グローバルアロケータを初期化する。
pub fn init(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_start + HEAP_SIZE;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let start_page = Page::containing_address(heap_start);
    let end_page = Page::containing_address(heap_end - 1u64);

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator
            .allocate_frame()
            .expect("out of physical memory while mapping heap");
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .expect("heap page mapping failed")
                .flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE as usize);
    }
}
