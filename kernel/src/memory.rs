use bootloader_api::info::{MemoryRegion, MemoryRegionKind, MemoryRegions};
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

/// CR3 レジスタからアクティブな Level 4 ページテーブルへの可変参照を取得する。
///
/// # Safety
/// - `phys_offset` はブートローダが設定した物理メモリオフセットでなければならない。
/// - この関数はカーネル初期化時に一度だけ呼ぶこと。
///   複数の `&mut` 参照が同時に存在すると未定義動作になる。
unsafe fn active_level_4_table(phys_offset: VirtAddr) -> &'static mut PageTable {
    let (l4_frame, _) = Cr3::read();
    let phys_addr = l4_frame.start_address();
    let virt_addr = phys_offset + phys_addr.as_u64();
    let table: *mut PageTable = virt_addr.as_mut_ptr();
    unsafe { &mut *table }
}

/// 物理メモリオフセットを用いて `OffsetPageTable` を初期化する。
///
/// # Safety
/// - `phys_offset` はブートローダが設定した物理メモリオフセットでなければならない。
/// - カーネル初期化時に一度だけ呼ぶこと。
pub unsafe fn init_page_table(phys_offset: VirtAddr) -> OffsetPageTable<'static> {
    let l4_table = unsafe { active_level_4_table(phys_offset) };
    unsafe { OffsetPageTable::new(l4_table, phys_offset) }
}

/// Usable なメモリリージョンから 4KiB フレームを順に払い出すアロケータ。
///
/// 解放には対応しない。ページテーブル構築など初期段階の用途を想定する。
pub struct BootFrameAllocator {
    usable_frames: UsableFrameIter,
}

impl BootFrameAllocator {
    pub fn new(memory_regions: &'static MemoryRegions) -> Self {
        Self {
            usable_frames: UsableFrameIter::new(memory_regions),
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.usable_frames.next()
    }
}

/// Usable リージョンを走査し、4KiB アラインされたフレームを列挙するイテレータ。
struct UsableFrameIter {
    regions: &'static [MemoryRegion],
    region_idx: usize,
    next_addr: u64,
}

impl UsableFrameIter {
    fn new(memory_regions: &'static MemoryRegions) -> Self {
        let mut iter = Self {
            regions: memory_regions,
            region_idx: 0,
            next_addr: 0,
        };
        iter.advance_to_usable();
        iter
    }

    /// 現在の region_idx が Usable リージョンを指すまで進め、
    /// next_addr をそのリージョンの先頭（4KiB アライン済み）に設定する。
    fn advance_to_usable(&mut self) {
        while self.region_idx < self.regions.len() {
            let region = &self.regions[self.region_idx];
            if region.kind == MemoryRegionKind::Usable {
                let aligned_start = align_up(region.start, 4096);
                if aligned_start < region.end {
                    self.next_addr = self.next_addr.max(aligned_start);
                    return;
                }
            }
            self.region_idx += 1;
        }
    }
}

impl Iterator for UsableFrameIter {
    type Item = PhysFrame<Size4KiB>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.region_idx >= self.regions.len() {
                return None;
            }

            let region = &self.regions[self.region_idx];
            if self.next_addr + 4096 <= region.end {
                let frame_addr = self.next_addr;
                self.next_addr += 4096;
                return Some(PhysFrame::containing_address(PhysAddr::new(frame_addr)));
            }

            self.region_idx += 1;
            self.advance_to_usable();
        }
    }
}

const fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
