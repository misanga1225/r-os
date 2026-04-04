use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use font8x8::UnicodeFonts;
use spin::Mutex;

pub const DEFAULT_TITLE_BAR_HEIGHT: usize = 24;
pub const WINDOW_PADDING: usize = 6;

// ---------------------------------------------------------------------------
// Memory region types (for memory map panel)
// ---------------------------------------------------------------------------

pub const MAX_REGIONS: usize = 32;

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
            MemRegionKind::Usable => (0x50, 0xC8, 0x78),
            MemRegionKind::Reserved => (0xE0, 0x6C, 0x5C),
            MemRegionKind::AcpiReclaimable => (0xE0, 0xA0, 0x50),
            MemRegionKind::AcpiNvs => (0xD0, 0x80, 0x40),
            MemRegionKind::BadMemory => (0xC0, 0x30, 0x30),
            MemRegionKind::Bootloader => (0x5C, 0x9C, 0xE0),
            MemRegionKind::Heap => (0xE0, 0xD0, 0x50),
            MemRegionKind::FrameBuffer => (0xB0, 0x70, 0xD0),
        }
    }
}

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

// ---------------------------------------------------------------------------
// Font constants
// ---------------------------------------------------------------------------

pub const FONT_SCALE_X: usize = 1;
pub const FONT_SCALE_Y: usize = 2;
pub const FONT_WIDTH: usize = 8 * FONT_SCALE_X;
pub const FONT_HEIGHT: usize = 8 * FONT_SCALE_Y;

// ---------------------------------------------------------------------------
// Pixel-level drawing (public, operate on arbitrary buffers)
// ---------------------------------------------------------------------------

pub fn put_pixel(buf: &mut [u8], info: FrameBufferInfo, x: usize, y: usize, r: u8, g: u8, b: u8) {
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

pub fn read_pixel(buf: &[u8], info: FrameBufferInfo, x: usize, y: usize) -> (u8, u8, u8) {
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

pub fn fill_rect(
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

pub fn draw_rect_outline(buf: &mut [u8], info: FrameBufferInfo, rect: Rect, r: u8, g: u8, b: u8) {
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

pub fn fill_vertical_gradient(
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

pub fn draw_background(buf: &mut [u8], info: FrameBufferInfo) {
    let screen = Rect {
        x: 0,
        y: 0,
        width: info.width,
        height: info.height,
    };
    fill_vertical_gradient(buf, info, screen, (0x1E, 0x22, 0x2B), (0x12, 0x14, 0x1A));

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
// Font / text drawing
// ---------------------------------------------------------------------------

pub fn draw_char_at(
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

pub fn draw_text(
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
// Console (public, no buffer ownership — used by WM windows)
// ---------------------------------------------------------------------------

pub struct Console {
    pub content: Rect,
    pub col: usize,
    pub row: usize,
    pub cols: usize,
    pub rows: usize,
    pub cursor_visible: bool,
}

impl Console {
    pub fn new(buf: &mut [u8], info: FrameBufferInfo, content: Rect) -> Self {
        let cols = content.width / FONT_WIDTH;
        let rows = content.height / FONT_HEIGHT;
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

    pub fn clear(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
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

    pub fn write_char(&mut self, buf: &mut [u8], info: FrameBufferInfo, c: char) {
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

    pub fn draw_cursor(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        if !self.cursor_visible {
            let x = self.content.x + self.col * FONT_WIDTH;
            let y0 = self.content.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(buf, info, x, y0 + dy, 0xFF, 0xFF, 0xFF);
            }
            self.cursor_visible = true;
        }
    }

    pub fn erase_cursor(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
        if self.cursor_visible {
            let x = self.content.x + self.col * FONT_WIDTH;
            let y0 = self.content.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(buf, info, x, y0 + dy, 0, 0, 0);
            }
            self.cursor_visible = false;
        }
    }

    pub fn scroll_up(&mut self, buf: &mut [u8], info: FrameBufferInfo) {
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

    pub fn write_str(&mut self, buf: &mut [u8], info: FrameBufferInfo, s: &str) {
        for c in s.chars() {
            self.write_char(buf, info, c);
        }
    }
}

// ---------------------------------------------------------------------------
// Mouse cursor sprite (public, used by WM)
// ---------------------------------------------------------------------------

pub const CURSOR_W: usize = 12;
pub const CURSOR_H: usize = 16;

#[rustfmt::skip]
pub const CURSOR_SPRITE: [[u8; CURSOR_W]; CURSOR_H] = [
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

// ---------------------------------------------------------------------------
// Global framebuffer state (simplified: only raw buffer + info)
// ---------------------------------------------------------------------------

pub(crate) struct FrameBufferState {
    pub(crate) buf: &'static mut [u8],
    pub(crate) info: FrameBufferInfo,
}

pub(crate) static FB_STATE: Mutex<Option<FrameBufferState>> = Mutex::new(None);

/// Initialize global framebuffer state. Call once at boot.
pub fn init(buf: &'static mut [u8], info: FrameBufferInfo) {
    draw_background(buf, info);
    *FB_STATE.lock() = Some(FrameBufferState { buf, info });
}

/// Access the raw framebuffer. The closure receives (buf, info).
/// Used by the window manager for compositing.
pub fn with_fb<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut [u8], FrameBufferInfo) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(state) = FB_STATE.lock().as_mut() {
            Some(f(state.buf, state.info))
        } else {
            None
        }
    })
}

/// Get framebuffer info (resolution, pixel format, etc.)
pub fn info() -> Option<FrameBufferInfo> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        FB_STATE.lock().as_ref().map(|s| s.info)
    })
}

// ---------------------------------------------------------------------------
// Formatting helpers (public, for memory map)
// ---------------------------------------------------------------------------

pub fn format_size(buf: &mut [u8; 24], size: u64) -> usize {
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

pub fn format_addr_range(buf: &mut [u8; 40], start: u64, end: u64) -> usize {
    let mut pos = 0;
    pos = write_hex(buf, pos, start, 40);
    if pos + 1 <= 40 {
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
