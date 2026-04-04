extern crate alloc;

use spin::Mutex;

use crate::framebuffer::{
    self, FONT_HEIGHT, FONT_WIDTH, MAX_REGIONS, MemRegionInfo, MemRegionKind,
};
use crate::{task, wm};

// ---------------------------------------------------------------------------
// Stored memory regions (set from main.rs before task starts)
// ---------------------------------------------------------------------------

static MEM_REGIONS: Mutex<([MemRegionInfo; MAX_REGIONS], usize)> = Mutex::new((
    [MemRegionInfo {
        start: 0,
        end: 0,
        kind: MemRegionKind::Usable,
    }; MAX_REGIONS],
    0,
));

pub fn set_memory_regions(regions: &[MemRegionInfo]) {
    let mut guard = MEM_REGIONS.lock();
    let count = regions.len().min(MAX_REGIONS);
    guard.0[..count].copy_from_slice(&regions[..count]);
    guard.1 = count;
}

/// Memory map task entry point.
pub fn task_main() -> ! {
    let win_id = task::current_window_id().unwrap_or(0);

    // Draw the memory map into our window buffer
    draw_memmap(win_id);
    wm::mark_dirty(win_id);

    // Memory map is static — just yield forever
    loop {
        task::yield_now();
    }
}

fn draw_memmap(win_id: usize) {
    wm::with_wm(|wm| {
        let win = match wm.window_mut(win_id) {
            Some(w) => w,
            None => return,
        };

        let content = win.content_rect();
        let buf = &mut win.buf;
        let info = win.buf_info;

        let guard = MEM_REGIONS.lock();
        let count = guard.1;
        if count == 0 {
            return;
        }
        let mut regions = [MemRegionInfo {
            start: 0,
            end: 0,
            kind: MemRegionKind::Usable,
        }; MAX_REGIONS];
        regions[..count].copy_from_slice(&guard.0[..count]);
        drop(guard);

        // Header
        let mut cy = content.y;
        framebuffer::draw_text(
            buf,
            info,
            content.x,
            cy,
            "Physical Memory",
            0xA0,
            0xA8,
            0xB4,
        );
        cy += FONT_HEIGHT + 4;
        framebuffer::fill_rect(buf, info, content.x, cy, content.width, 1, 0x33, 0x38, 0x44);
        cy += 4;

        // Region list
        let row_height = FONT_HEIGHT * 2 + 8;
        let legend_kinds = [
            MemRegionKind::Usable,
            MemRegionKind::Reserved,
            MemRegionKind::AcpiReclaimable,
            MemRegionKind::AcpiNvs,
            MemRegionKind::Bootloader,
            MemRegionKind::Heap,
            MemRegionKind::FrameBuffer,
        ];
        let legend_rows = (legend_kinds.len() + 1) / 2;
        let legend_height = legend_rows * (FONT_HEIGHT + 4) + 8;

        let list_area = content
            .height
            .saturating_sub(legend_height + (cy - content.y));
        let max_visible = list_area / row_height;
        let visible = count.min(max_visible);

        for i in 0..visible {
            let region = &regions[i];
            let (cr, cg, cb) = region.kind.color();
            let size = region.end - region.start;

            // Color bar
            framebuffer::fill_rect(buf, info, content.x, cy, content.width, 3, cr, cg, cb);
            cy += 4;

            // Line 1: kind + size
            framebuffer::draw_text(
                buf,
                info,
                content.x + 2,
                cy,
                region.kind.label(),
                cr,
                cg,
                cb,
            );
            let mut size_buf = [0u8; 24];
            let size_len = framebuffer::format_size(&mut size_buf, size);
            let size_str = core::str::from_utf8(&size_buf[..size_len]).unwrap_or("");
            let size_x = (content.x + content.width).saturating_sub(size_len * FONT_WIDTH + 2);
            framebuffer::draw_text(buf, info, size_x, cy, size_str, 0xCC, 0xCC, 0xCC);
            cy += FONT_HEIGHT;

            // Line 2: address range
            let mut addr_buf = [0u8; 40];
            let addr_len = framebuffer::format_addr_range(&mut addr_buf, region.start, region.end);
            let addr_str = core::str::from_utf8(&addr_buf[..addr_len]).unwrap_or("");
            framebuffer::draw_text(buf, info, content.x + 2, cy, addr_str, 0x88, 0x8C, 0x96);
            cy += FONT_HEIGHT;

            // Separator
            cy += 1;
            framebuffer::fill_rect(buf, info, content.x, cy, content.width, 1, 0x1A, 0x1E, 0x28);
            cy += 3;
        }

        // Legend
        let legend_y = content.y + content.height - legend_height;
        framebuffer::fill_rect(
            buf,
            info,
            content.x,
            legend_y,
            content.width,
            1,
            0x33,
            0x38,
            0x44,
        );

        let cols = 2;
        let col_width = content.width / cols;
        for (i, &kind) in legend_kinds.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let lx = content.x + col * col_width;
            let ly = legend_y + 6 + row * (FONT_HEIGHT + 4);
            let (cr, cg, cb) = kind.color();
            framebuffer::fill_rect(buf, info, lx, ly + 2, 12, 12, cr, cg, cb);
            framebuffer::draw_text(buf, info, lx + 16, ly, kind.label(), 0xAA, 0xAA, 0xAA);
        }

        win.dirty = true;
    });
}
