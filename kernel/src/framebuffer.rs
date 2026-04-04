use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use core::fmt;
use font8x8::UnicodeFonts;
use spin::Mutex;

use crate::mouse::MouseEvent;

pub const DEFAULT_TITLE_BAR_HEIGHT: usize = 24;
const WINDOW_PADDING: usize = 6;

// ---------------------------------------------------------------------------
// Memory region types (for memory map panel)
// ---------------------------------------------------------------------------

const MAX_REGIONS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MemRegionKind {
    Usable,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadMemory,
    Bootloader,
    Heap,
    FrameBuffer,
}

impl MemRegionKind {
    pub fn label(self) -> &'static str {
        match self {
            MemRegionKind::Usable => "Usable",
            MemRegionKind::Reserved => "Reserved",
            MemRegionKind::AcpiReclaimable => "ACPI Reclaim",
            MemRegionKind::AcpiNvs => "ACPI NVS",
            MemRegionKind::BadMemory => "Bad Memory",
            MemRegionKind::Bootloader => "Bootloader",
            MemRegionKind::Heap => "Heap",
            MemRegionKind::FrameBuffer => "FrameBuffer",
        }
    }

    pub fn color(self) -> (u8, u8, u8) {
        match self {
            MemRegionKind::Usable => (0x50, 0xC8, 0x78),   // green
            MemRegionKind::Reserved => (0xE0, 0x6C, 0x5C), // red-orange
            MemRegionKind::AcpiReclaimable => (0xE0, 0xA0, 0x50), // orange
            MemRegionKind::AcpiNvs => (0xD0, 0x80, 0x40),  // dark orange
            MemRegionKind::BadMemory => (0xC0, 0x30, 0x30), // dark red
            MemRegionKind::Bootloader => (0x5C, 0x9C, 0xE0), // blue
            MemRegionKind::Heap => (0xE0, 0xD0, 0x50),     // yellow
            MemRegionKind::FrameBuffer => (0xB0, 0x70, 0xD0), // purple
        }
    }
}

/// E820 BIOS メモリタイプからの変換
pub fn bios_e820_to_kind(tag: u32) -> MemRegionKind {
    match tag {
        1 => MemRegionKind::Usable,
        2 => MemRegionKind::Reserved,
        3 => MemRegionKind::AcpiReclaimable,
        4 => MemRegionKind::AcpiNvs,
        5 => MemRegionKind::BadMemory,
        _ => MemRegionKind::Reserved,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MemRegionInfo {
    pub start: u64,
    pub end: u64,
    pub kind: MemRegionKind,
}

static MEM_REGIONS: Mutex<([MemRegionInfo; MAX_REGIONS], usize)> = Mutex::new((
    [MemRegionInfo {
        start: 0,
        end: 0,
        kind: MemRegionKind::Usable,
    }; MAX_REGIONS],
    0,
));

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title_bar_height: usize,
}

impl Window {
    fn title_bar_rect(self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.title_bar_height.min(self.height),
        }
    }

    fn content_rect(self) -> Rect {
        let title_h = self.title_bar_height.min(self.height);
        let x = self.x + WINDOW_PADDING;
        let y = self.y + title_h + WINDOW_PADDING;
        let width = self.width.saturating_sub(WINDOW_PADDING * 2);
        let height = self.height.saturating_sub(title_h + WINDOW_PADDING * 2);
        Rect {
            x,
            y,
            width,
            height,
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel-level drawing (free functions operating on raw buffer)
// ---------------------------------------------------------------------------

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
            let len = bpp.min(4);
            buf[offset..(offset + len)].copy_from_slice(&bytes[..len]);
        }
        _ => {}
    }
}

fn read_pixel(buf: &[u8], info: FrameBufferInfo, x: usize, y: usize) -> (u8, u8, u8) {
    if x >= info.width || y >= info.height {
        return (0, 0, 0);
    }

    let bpp = info.bytes_per_pixel;
    let offset = (y * info.stride + x) * bpp;

    if offset + bpp > buf.len() {
        return (0, 0, 0);
    }

    match info.pixel_format {
        PixelFormat::Rgb => (buf[offset], buf[offset + 1], buf[offset + 2]),
        PixelFormat::Bgr => (buf[offset + 2], buf[offset + 1], buf[offset]),
        PixelFormat::U8 => {
            let v = buf[offset];
            (v, v, v)
        }
        PixelFormat::Unknown {
            red_position,
            green_position,
            blue_position,
        } => {
            let mut raw: u32 = 0;
            let len = bpp.min(4);
            for i in 0..len {
                raw |= (buf[offset + i] as u32) << (i * 8);
            }
            let r = ((raw >> red_position) & 0xFF) as u8;
            let g = ((raw >> green_position) & 0xFF) as u8;
            let b = ((raw >> blue_position) & 0xFF) as u8;
            (r, g, b)
        }
        _ => (0, 0, 0),
    }
}

fn fill_rect(
    buf: &mut [u8],
    info: FrameBufferInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    r: u8,
    g: u8,
    b: u8,
) {
    let x_end = x.saturating_add(width).min(info.width);
    let y_end = y.saturating_add(height).min(info.height);
    for py in y..y_end {
        for px in x..x_end {
            put_pixel(buf, info, px, py, r, g, b);
        }
    }
}

fn draw_rect_outline(buf: &mut [u8], info: FrameBufferInfo, rect: Rect, r: u8, g: u8, b: u8) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let x_end = rect.x.saturating_add(rect.width).min(info.width);
    let y_end = rect.y.saturating_add(rect.height).min(info.height);
    let x_last = x_end.saturating_sub(1);
    let y_last = y_end.saturating_sub(1);

    for x in rect.x..=x_last {
        put_pixel(buf, info, x, rect.y, r, g, b);
        put_pixel(buf, info, x, y_last, r, g, b);
    }
    for y in rect.y..=y_last {
        put_pixel(buf, info, rect.x, y, r, g, b);
        put_pixel(buf, info, x_last, y, r, g, b);
    }
}

fn fill_vertical_gradient(
    buf: &mut [u8],
    info: FrameBufferInfo,
    rect: Rect,
    top: (u8, u8, u8),
    bottom: (u8, u8, u8),
) {
    if rect.height == 0 {
        return;
    }
    let y_end = rect.y.saturating_add(rect.height).min(info.height);
    let denom = rect.height.saturating_sub(1).max(1) as u32;
    for (i, y) in (rect.y..y_end).enumerate() {
        let t = (i as u32) * 255 / denom;
        let r = lerp_u8(top.0, bottom.0, t);
        let g = lerp_u8(top.1, bottom.1, t);
        let b = lerp_u8(top.2, bottom.2, t);
        fill_rect(buf, info, rect.x, y, rect.width, 1, r, g, b);
    }
}

fn lerp_u8(a: u8, b: u8, t: u32) -> u8 {
    let a = a as i32;
    let b = b as i32;
    let t = t as i32;
    let value = a + (b - a) * t / 255;
    value.clamp(0, 255) as u8
}

fn draw_background(buf: &mut [u8], info: FrameBufferInfo) {
    let screen = Rect {
        x: 0,
        y: 0,
        width: info.width,
        height: info.height,
    };
    fill_vertical_gradient(buf, info, screen, (0x1E, 0x22, 0x2B), (0x12, 0x14, 0x1A));

    // Subtle dot pattern.
    let step = 4;
    for y in (0..info.height).step_by(step) {
        for x in (0..info.width).step_by(step) {
            if ((x + y) & 0x0F) == 0 {
                put_pixel(buf, info, x, y, 0x2A, 0x2E, 0x38);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Font / text drawing (free functions)
// ---------------------------------------------------------------------------

const FONT_SCALE_X: usize = 1;
const FONT_SCALE_Y: usize = 2;
const FONT_WIDTH: usize = 8 * FONT_SCALE_X;
const FONT_HEIGHT: usize = 8 * FONT_SCALE_Y;

fn draw_char_at(
    buf: &mut [u8],
    info: FrameBufferInfo,
    x0: usize,
    y0: usize,
    c: char,
    r: u8,
    g: u8,
    b: u8,
) {
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or(font8x8::BASIC_FONTS.get('?').unwrap());

    for (gly_y, &byte) in glyph.iter().enumerate() {
        for gly_x in 0..8 {
            let on = byte & (1 << gly_x) != 0;
            if !on {
                continue;
            }
            for sy in 0..FONT_SCALE_Y {
                for sx in 0..FONT_SCALE_X {
                    put_pixel(
                        buf,
                        info,
                        x0 + gly_x * FONT_SCALE_X + sx,
                        y0 + gly_y * FONT_SCALE_Y + sy,
                        r,
                        g,
                        b,
                    );
                }
            }
        }
    }
}

fn draw_text(
    buf: &mut [u8],
    info: FrameBufferInfo,
    x: usize,
    y: usize,
    text: &str,
    r: u8,
    g: u8,
    b: u8,
) {
    let mut cx = x;
    for c in text.chars() {
        draw_char_at(buf, info, cx, y, c, r, g, b);
        cx = cx.saturating_add(FONT_WIDTH);
    }
}

// ---------------------------------------------------------------------------
// Console (text state only — no longer owns the buffer)
// ---------------------------------------------------------------------------

struct Console {
    content: Rect,
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
    cursor_visible: bool,
}

impl Console {
    fn new(buf: &mut [u8], info: FrameBufferInfo, content: Rect) -> Self {
        let cols = content.width / FONT_WIDTH;
        let rows = content.height / FONT_HEIGHT;
        // Clear the content area
        fill_rect(
            buf,
            info,
            content.x,
            content.y,
            content.width,
            content.height,
            0,
            0,
            0,
        );
        Console {
            content,
            col: 0,
            row: 0,
            cols,
            rows,
            cursor_visible: false,
        }
    }

    fn clear(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        fill_rect(
            buf,
            info,
            self.content.x,
            self.content.y,
            self.content.width,
            self.content.height,
            0,
            0,
            0,
        );
        self.col = 0;
        self.row = 0;
    }

    fn write_char(&mut self, buf: &mut [u8], info: FrameBufferInfo, c: char) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        match c {
            '\n' => {
                self.col = 0;
                self.row += 1;
                if self.row >= self.rows {
                    self.scroll_up(buf, info);
                }
            }
            '\r' => {
                self.col = 0;
            }
            '\u{8}' => {
                if self.col > 0 {
                    self.col -= 1;
                    self.clear_char(buf, info, self.col, self.row);
                }
            }
            c => {
                if self.col >= self.cols {
                    self.col = 0;
                    self.row += 1;
                    if self.row >= self.rows {
                        self.scroll_up(buf, info);
                    }
                }
                self.draw_char(buf, info, c, self.col, self.row);
                self.col += 1;
            }
        }
    }

    fn draw_char(&self, buf: &mut [u8], info: FrameBufferInfo, c: char, col: usize, row: usize) {
        let x0 = self.content.x + col * FONT_WIDTH;
        let y0 = self.content.y + row * FONT_HEIGHT;
        draw_char_at(buf, info, x0, y0, c, 0xFF, 0xFF, 0xFF);
    }

    fn clear_char(&self, buf: &mut [u8], info: FrameBufferInfo, col: usize, row: usize) {
        let x0 = self.content.x + col * FONT_WIDTH;
        let y0 = self.content.y + row * FONT_HEIGHT;
        for dy in 0..FONT_HEIGHT {
            for dx in 0..FONT_WIDTH {
                put_pixel(buf, info, x0 + dx, y0 + dy, 0, 0, 0);
            }
        }
    }

    fn draw_cursor(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        if !self.cursor_visible {
            let x = self.content.x + self.col * FONT_WIDTH;
            let y0 = self.content.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(buf, info, x, y0 + dy, 0xFF, 0xFF, 0xFF);
            }
            self.cursor_visible = true;
        }
    }

    fn erase_cursor(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        if self.cursor_visible {
            let x = self.content.x + self.col * FONT_WIDTH;
            let y0 = self.content.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(buf, info, x, y0 + dy, 0, 0, 0);
            }
            self.cursor_visible = false;
        }
    }

    fn scroll_up(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        if self.rows == 0 {
            return;
        }

        let bpp = info.bytes_per_pixel;
        let row_bytes = self.content.width * bpp;
        let content_y_end = self.content.y + self.content.height;

        let max_copy_rows = self.content.height.saturating_sub(FONT_HEIGHT);
        for wy in 0..max_copy_rows {
            let src_y = self.content.y + wy + FONT_HEIGHT;
            let dst_y = self.content.y + wy;
            if src_y + 1 > content_y_end {
                break;
            }
            let src_start = (src_y * info.stride + self.content.x) * bpp;
            let dst_start = (dst_y * info.stride + self.content.x) * bpp;
            if src_start + row_bytes <= buf.len() && dst_start + row_bytes <= buf.len() {
                buf.copy_within(src_start..(src_start + row_bytes), dst_start);
            }
        }

        let clear_y_start = content_y_end.saturating_sub(FONT_HEIGHT);
        fill_rect(
            buf,
            info,
            self.content.x,
            clear_y_start,
            self.content.width,
            FONT_HEIGHT,
            0,
            0,
            0,
        );

        self.row = self.rows - 1;
    }

    fn write_str(&mut self, buf: &mut [u8], info: FrameBufferInfo, s: &str) {
        for c in s.chars() {
            self.write_char(buf, info, c);
        }
    }
}

// ---------------------------------------------------------------------------
// Mouse cursor
// ---------------------------------------------------------------------------

const CURSOR_W: usize = 12;
const CURSOR_H: usize = 16;

// 0 = transparent, 1 = black (outline), 2 = white (fill)
#[rustfmt::skip]
const CURSOR_SPRITE: [[u8; CURSOR_W]; CURSOR_H] = [
    [1,0,0,0,0,0,0,0,0,0,0,0],
    [1,1,0,0,0,0,0,0,0,0,0,0],
    [1,2,1,0,0,0,0,0,0,0,0,0],
    [1,2,2,1,0,0,0,0,0,0,0,0],
    [1,2,2,2,1,0,0,0,0,0,0,0],
    [1,2,2,2,2,1,0,0,0,0,0,0],
    [1,2,2,2,2,2,1,0,0,0,0,0],
    [1,2,2,2,2,2,2,1,0,0,0,0],
    [1,2,2,2,2,2,2,2,1,0,0,0],
    [1,2,2,2,2,2,2,2,2,1,0,0],
    [1,2,2,2,2,2,1,1,1,1,1,0],
    [1,2,2,1,2,2,1,0,0,0,0,0],
    [1,2,1,0,1,2,2,1,0,0,0,0],
    [1,1,0,0,1,2,2,1,0,0,0,0],
    [1,0,0,0,0,1,2,2,1,0,0,0],
    [0,0,0,0,0,1,1,1,0,0,0,0],
];

struct CursorState {
    x: i32,
    y: i32,
    screen_width: i32,
    screen_height: i32,
    visible: bool,
    saved_bg: [[u8; 3]; CURSOR_W * CURSOR_H],
}

impl CursorState {
    fn new(screen_width: usize, screen_height: usize) -> Self {
        CursorState {
            x: (screen_width / 2) as i32,
            y: (screen_height / 2) as i32,
            screen_width: screen_width as i32,
            screen_height: screen_height as i32,
            visible: false,
            saved_bg: [[0; 3]; CURSOR_W * CURSOR_H],
        }
    }

    fn save_background(&mut self, buf: &[u8], info: FrameBufferInfo) {
        let cx = self.x as usize;
        let cy = self.y as usize;
        for sy in 0..CURSOR_H {
            for sx in 0..CURSOR_W {
                let (r, g, b) = read_pixel(buf, info, cx + sx, cy + sy);
                self.saved_bg[sy * CURSOR_W + sx] = [r, g, b];
            }
        }
    }

    fn restore_background(&self, buf: &mut [u8], info: FrameBufferInfo) {
        if !self.visible {
            return;
        }
        let cx = self.x as usize;
        let cy = self.y as usize;
        for sy in 0..CURSOR_H {
            for sx in 0..CURSOR_W {
                if CURSOR_SPRITE[sy][sx] != 0 {
                    let [r, g, b] = self.saved_bg[sy * CURSOR_W + sx];
                    put_pixel(buf, info, cx + sx, cy + sy, r, g, b);
                }
            }
        }
    }

    fn draw(&self, buf: &mut [u8], info: FrameBufferInfo) {
        let cx = self.x as usize;
        let cy = self.y as usize;
        for sy in 0..CURSOR_H {
            for sx in 0..CURSOR_W {
                match CURSOR_SPRITE[sy][sx] {
                    1 => put_pixel(buf, info, cx + sx, cy + sy, 0x00, 0x00, 0x00),
                    2 => put_pixel(buf, info, cx + sx, cy + sy, 0xFF, 0xFF, 0xFF),
                    _ => {}
                }
            }
        }
    }

    fn update(&mut self, buf: &mut [u8], info: FrameBufferInfo, event: MouseEvent) {
        // 旧位置の背景を復元
        self.restore_background(buf, info);

        // 座標更新（PS/2のY軸は上が正なので反転）
        self.x += event.dx as i32;
        self.y -= event.dy as i32;
        self.x = self.x.clamp(0, self.screen_width - 1);
        self.y = self.y.clamp(0, self.screen_height - 1);

        // 新位置の背景を保存して描画
        self.save_background(buf, info);
        self.draw(buf, info);
        self.visible = true;
    }

    fn show(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        self.save_background(buf, info);
        self.draw(buf, info);
        self.visible = true;
    }
}

// ---------------------------------------------------------------------------
// Global framebuffer state
// ---------------------------------------------------------------------------

struct FrameBufferState {
    buf: &'static mut [u8],
    info: FrameBufferInfo,
    console: Console,
    cursor: Option<CursorState>,
}

/// fmt::Write adapter that borrows from FrameBufferState.
struct ConsoleWriter<'a> {
    state: &'a mut FrameBufferState,
}

impl<'a> fmt::Write for ConsoleWriter<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let buf = &mut *self.state.buf;
        let info = self.state.info;
        self.state.console.write_str(buf, info, s);
        Ok(())
    }
}

static FB_STATE: Mutex<Option<FrameBufferState>> = Mutex::new(None);

fn draw_window(
    buf: &mut [u8],
    info: FrameBufferInfo,
    window: Window,
    title: &str,
    title_bar_color: (u8, u8, u8),
) {
    fill_rect(
        buf,
        info,
        window.x,
        window.y,
        window.width,
        window.height,
        0,
        0,
        0,
    );

    let title_bar = window.title_bar_rect();
    fill_rect(
        buf,
        info,
        title_bar.x,
        title_bar.y,
        title_bar.width,
        title_bar.height,
        title_bar_color.0,
        title_bar_color.1,
        title_bar_color.2,
    );

    draw_rect_outline(
        buf,
        info,
        Rect {
            x: window.x,
            y: window.y,
            width: window.width,
            height: window.height,
        },
        0x55,
        0x5A,
        0x66,
    );

    if title_bar.height >= FONT_HEIGHT {
        let text_x = window.x + WINDOW_PADDING;
        let text_y = window.y + (title_bar.height - FONT_HEIGHT) / 2;
        draw_text(buf, info, text_x, text_y, title, 0xF0, 0xF2, 0xF6);
    }
}

pub fn init(buf: &'static mut [u8], info: FrameBufferInfo, window: Window, title: &str) {
    draw_background(buf, info);
    draw_window(buf, info, window, title, (0x18, 0x2A, 0x40));

    let content = window.content_rect();
    let console = Console::new(buf, info, content);

    *FB_STATE.lock() = Some(FrameBufferState {
        buf,
        info,
        console,
        cursor: None,
    });
}

// ---------------------------------------------------------------------------
// Taskbar
// ---------------------------------------------------------------------------

pub fn draw_taskbar() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            let buf = &mut *state.buf;
            let info = state.info;

            // Background
            fill_rect(buf, info, 0, 0, info.width, 28, 0x1A, 0x1E, 0x28);
            // Bottom border
            fill_rect(buf, info, 0, 27, info.width, 1, 0x33, 0x38, 0x44);
            // "r-os" label
            draw_text(buf, info, 12, 6, "r-os", 0xE0, 0xE4, 0xEC);
        }
    });
}

// ---------------------------------------------------------------------------
// Memory map panel
// ---------------------------------------------------------------------------

pub fn set_memory_regions(regions: &[MemRegionInfo]) {
    let mut guard = MEM_REGIONS.lock();
    let count = regions.len().min(MAX_REGIONS);
    for i in 0..count {
        guard.0[i] = regions[i];
    }
    guard.1 = count;
}

pub fn draw_memory_map_panel(window: Window) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            let buf = &mut *state.buf;
            let info = state.info;

            draw_window(buf, info, window, "Memory Map", (0x28, 0x1A, 0x38));

            let content = window.content_rect();
            let regions_guard = MEM_REGIONS.lock();
            let count = regions_guard.1;
            if count == 0 {
                return;
            }

            // Copy regions out of the lock
            let mut regions = [MemRegionInfo {
                start: 0,
                end: 0,
                kind: MemRegionKind::Usable,
            }; MAX_REGIONS];
            regions[..count].copy_from_slice(&regions_guard.0[..count]);
            drop(regions_guard);

            // --- Layout ---
            // Row per region: color bar (4px) + line1 (kind + size) + line2 (address range)
            // Then legend at the bottom
            let row_height = FONT_HEIGHT * 2 + 8; // 2 text lines + padding
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
            let total_header = FONT_HEIGHT + 6; // "Physical Memory" header

            // Compute how many regions fit in the list area
            let list_area = content.height.saturating_sub(legend_height + total_header);
            let max_visible = list_area / row_height;

            let mut cy = content.y;

            // --- Header ---
            draw_text(
                buf,
                info,
                content.x,
                cy,
                "Physical Memory",
                0xA0,
                0xA8,
                0xB4,
            );
            cy += FONT_HEIGHT + 6;

            // Separator line
            fill_rect(buf, info, content.x, cy, content.width, 1, 0x33, 0x38, 0x44);
            cy += 4;

            // --- Region list ---
            let visible = count.min(max_visible);
            for i in 0..visible {
                let region = &regions[i];
                let (cr, cg, cb) = region.kind.color();
                let size = region.end - region.start;

                // Color indicator bar (full width, 3px tall)
                fill_rect(buf, info, content.x, cy, content.width, 3, cr, cg, cb);
                cy += 4;

                // Line 1: kind label + size
                let kind_label = region.kind.label();
                draw_text(buf, info, content.x + 2, cy, kind_label, cr, cg, cb);

                // Size text (right-aligned area after kind label)
                let mut size_buf = [0u8; 24];
                let size_len = format_size(&mut size_buf, size);
                let size_str = core::str::from_utf8(&size_buf[..size_len]).unwrap_or("");
                let size_text_w = size_len * FONT_WIDTH;
                let size_x = (content.x + content.width).saturating_sub(size_text_w + 2);
                draw_text(buf, info, size_x, cy, size_str, 0xCC, 0xCC, 0xCC);
                cy += FONT_HEIGHT;

                // Line 2: address range "0xSTART - 0xEND"
                let mut addr_buf = [0u8; 40];
                let addr_len = format_addr_range(&mut addr_buf, region.start, region.end);
                let addr_str = core::str::from_utf8(&addr_buf[..addr_len]).unwrap_or("");
                draw_text(buf, info, content.x + 2, cy, addr_str, 0x88, 0x8C, 0x96);
                cy += FONT_HEIGHT;

                // Row separator
                cy += 1;
                fill_rect(buf, info, content.x, cy, content.width, 1, 0x1A, 0x1E, 0x28);
                cy += 3;
            }

            // --- Legend ---
            let legend_y = content.y + content.height - legend_height;
            // Separator above legend
            fill_rect(
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
                fill_rect(buf, info, lx, ly + 2, 12, 12, cr, cg, cb);
                draw_text(buf, info, lx + 16, ly, kind.label(), 0xAA, 0xAA, 0xAA);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Formatting helpers for memory map
// ---------------------------------------------------------------------------

fn format_size(buf: &mut [u8; 24], size: u64) -> usize {
    let (val, suffix) = if size >= 1024 * 1024 * 1024 {
        (size / (1024 * 1024 * 1024), " GiB")
    } else if size >= 1024 * 1024 {
        (size / (1024 * 1024), " MiB")
    } else if size >= 1024 {
        (size / 1024, " KiB")
    } else {
        (size, " B")
    };
    let mut pos = 0;
    pos = write_u64_decimal(buf, pos, val);
    for &c in suffix.as_bytes() {
        if pos < 24 {
            buf[pos] = c;
            pos += 1;
        }
    }
    pos
}

fn format_addr_range(buf: &mut [u8; 40], start: u64, end: u64) -> usize {
    let mut pos = 0;
    pos = write_hex(buf, pos, start, 40);
    // " - "
    if pos + 3 <= 40 {
        buf[pos] = b'-';
        pos += 1;
    }
    pos = write_hex(buf, pos, end, 40);
    pos
}

fn write_hex(buf: &mut [u8], mut pos: usize, val: u64, limit: usize) -> usize {
    if pos + 2 > limit {
        return pos;
    }
    buf[pos] = b'0';
    pos += 1;
    buf[pos] = b'x';
    pos += 1;

    // Find first non-zero nibble (min 4 hex digits)
    let mut started = false;
    for shift in (0..16).rev() {
        let nibble = ((val >> (shift * 4)) & 0xF) as u8;
        if nibble != 0 || started || shift < 4 {
            if pos < limit {
                buf[pos] = if nibble < 10 {
                    b'0' + nibble
                } else {
                    b'a' + nibble - 10
                };
                pos += 1;
                started = true;
            }
        }
    }
    pos
}

fn write_u64_decimal(buf: &mut [u8], mut pos: usize, val: u64) -> usize {
    if val == 0 {
        if pos < buf.len() {
            buf[pos] = b'0';
            pos += 1;
        }
        return pos;
    }
    let mut digits = [0u8; 20];
    let mut n = val;
    let mut dlen = 0;
    while n > 0 {
        digits[dlen] = (n % 10) as u8 + b'0';
        n /= 10;
        dlen += 1;
    }
    for d in (0..dlen).rev() {
        if pos < buf.len() {
            buf[pos] = digits[d];
            pos += 1;
        }
    }
    pos
}

pub fn init_cursor() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            let w = state.info.width;
            let h = state.info.height;
            let mut cursor = CursorState::new(w, h);
            cursor.show(state.buf, state.info);
            state.cursor = Some(cursor);
        }
    });
}

pub fn update_cursor(event: MouseEvent) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            if let Some(cursor) = state.cursor.as_mut() {
                cursor.update(state.buf, state.info, event);
            }
        }
    });
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            use fmt::Write;
            let mut writer = ConsoleWriter { state };
            writer.write_fmt(args).unwrap();
        }
    });
}

pub fn show_cursor() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            state.console.draw_cursor(state.buf, state.info);
        }
    });
}

pub fn hide_cursor() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            state.console.erase_cursor(state.buf, state.info);
        }
    });
}

pub fn clear() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            state.console.clear(state.buf, state.info);
        }
    });
}
