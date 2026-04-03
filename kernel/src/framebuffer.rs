use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use core::fmt;
use font8x8::UnicodeFonts;
use spin::Mutex;

pub const DEFAULT_TITLE_BAR_HEIGHT: usize = 24;
const WINDOW_PADDING: usize = 6;

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

/// Draw a single pixel at (x, y) in RGB.
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

// ---------------------------------------------------------------------------
// Framebuffer Text Console
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

struct Console {
    buf: &'static mut [u8],
    info: FrameBufferInfo,
    content: Rect,
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
    cursor_visible: bool,
}

impl Console {
    fn new(buf: &'static mut [u8], info: FrameBufferInfo, content: Rect) -> Self {
        let cols = content.width / FONT_WIDTH;
        let rows = content.height / FONT_HEIGHT;
        let mut console = Console {
            buf,
            info,
            content,
            col: 0,
            row: 0,
            cols,
            rows,
            cursor_visible: false,
        };
        console.clear();
        console
    }

    fn clear(&mut self) {
        fill_rect(
            self.buf,
            self.info,
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

    fn write_char(&mut self, c: char) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        match c {
            '\n' => {
                self.col = 0;
                self.row += 1;
                if self.row >= self.rows {
                    self.scroll_up();
                }
            }
            '\r' => {
                self.col = 0;
            }
            '\u{8}' => {
                // Backspace: move one column left and clear it.
                if self.col > 0 {
                    self.col -= 1;
                    self.clear_char(self.col, self.row);
                }
            }
            c => {
                if self.col >= self.cols {
                    self.col = 0;
                    self.row += 1;
                    if self.row >= self.rows {
                        self.scroll_up();
                    }
                }
                self.draw_char(c, self.col, self.row);
                self.col += 1;
            }
        }
    }

    fn draw_char(&mut self, c: char, col: usize, row: usize) {
        let x0 = self.content.x + col * FONT_WIDTH;
        let y0 = self.content.y + row * FONT_HEIGHT;
        draw_char_at(self.buf, self.info, x0, y0, c, 0xFF, 0xFF, 0xFF);
    }

    fn clear_char(&mut self, col: usize, row: usize) {
        let x0 = self.content.x + col * FONT_WIDTH;
        let y0 = self.content.y + row * FONT_HEIGHT;
        for dy in 0..FONT_HEIGHT {
            for dx in 0..FONT_WIDTH {
                put_pixel(self.buf, self.info, x0 + dx, y0 + dy, 0, 0, 0);
            }
        }
    }

    fn draw_cursor(&mut self) {
        if !self.cursor_visible {
            let x = self.content.x + self.col * FONT_WIDTH;
            let y0 = self.content.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(self.buf, self.info, x, y0 + dy, 0xFF, 0xFF, 0xFF);
            }
            self.cursor_visible = true;
        }
    }

    fn erase_cursor(&mut self) {
        if self.cursor_visible {
            let x = self.content.x + self.col * FONT_WIDTH;
            let y0 = self.content.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(self.buf, self.info, x, y0 + dy, 0, 0, 0);
            }
            self.cursor_visible = false;
        }
    }

    fn scroll_up(&mut self) {
        if self.rows == 0 {
            return;
        }

        let bpp = self.info.bytes_per_pixel;
        let row_bytes = self.content.width * bpp;
        let content_y_end = self.content.y + self.content.height;

        // Copy content up by one text row.
        let max_copy_rows = self.content.height.saturating_sub(FONT_HEIGHT);
        for wy in 0..max_copy_rows {
            let src_y = self.content.y + wy + FONT_HEIGHT;
            let dst_y = self.content.y + wy;
            if src_y + 1 > content_y_end {
                break;
            }
            let src_start = (src_y * self.info.stride + self.content.x) * bpp;
            let dst_start = (dst_y * self.info.stride + self.content.x) * bpp;
            if src_start + row_bytes <= self.buf.len() && dst_start + row_bytes <= self.buf.len() {
                self.buf
                    .copy_within(src_start..(src_start + row_bytes), dst_start);
            }
        }

        // Clear the last text row inside the content area.
        let clear_y_start = content_y_end.saturating_sub(FONT_HEIGHT);
        fill_rect(
            self.buf,
            self.info,
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
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Global Console
// ---------------------------------------------------------------------------

static CONSOLE: Mutex<Option<Console>> = Mutex::new(None);

pub fn init(buf: &'static mut [u8], info: FrameBufferInfo, window: Window, title: &str) {
    draw_background(buf, info);

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
        0x18,
        0x2A,
        0x40,
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

    let content = window.content_rect();
    *CONSOLE.lock() = Some(Console::new(buf, info, content));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(console) = CONSOLE.lock().as_mut() {
            console.write_fmt(args).unwrap();
        }
    });
}

pub fn show_cursor() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(console) = CONSOLE.lock().as_mut() {
            console.draw_cursor();
        }
    });
}

pub fn hide_cursor() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(console) = CONSOLE.lock().as_mut() {
            console.erase_cursor();
        }
    });
}

pub fn clear() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(console) = CONSOLE.lock().as_mut() {
            console.clear();
        }
    });
}
