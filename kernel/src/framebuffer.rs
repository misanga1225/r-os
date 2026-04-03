use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use core::fmt;
use font8x8::UnicodeFonts;
use spin::Mutex;

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
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

const FONT_SCALE_X: usize = 1; // Font scale X
const FONT_SCALE_Y: usize = 2; // Font scale Y
const FONT_WIDTH: usize = 8 * FONT_SCALE_X;
const FONT_HEIGHT: usize = 8 * FONT_SCALE_Y;

struct Console {
    buf: &'static mut [u8],
    info: FrameBufferInfo,
    window: Window,
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
    cursor_visible: bool,
}

impl Console {
    fn new(buf: &'static mut [u8], info: FrameBufferInfo, window: Window) -> Self {
        let cols = window.width / FONT_WIDTH;
        let rows = window.height / FONT_HEIGHT;
        let mut console = Console {
            buf,
            info,
            window,
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
            self.window.x,
            self.window.y,
            self.window.width,
            self.window.height,
            0,
            0,
            0,
        );
        self.col = 0;
        self.row = 0;
    }

    fn write_char(&mut self, c: char) {
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
        let glyph = font8x8::BASIC_FONTS
            .get(c)
            .unwrap_or(font8x8::BASIC_FONTS.get('?').unwrap());

        let x0 = self.window.x + col * FONT_WIDTH;
        let y0 = self.window.y + row * FONT_HEIGHT;

        for (gly_y, &byte) in glyph.iter().enumerate() {
            for gly_x in 0..8 {
                let on = byte & (1 << gly_x) != 0;
                let (r, g, b) = if on { (0xFF, 0xFF, 0xFF) } else { (0, 0, 0) };
                for sy in 0..FONT_SCALE_Y {
                    for sx in 0..FONT_SCALE_X {
                        put_pixel(
                            self.buf,
                            self.info,
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

    fn clear_char(&mut self, col: usize, row: usize) {
        let x0 = self.window.x + col * FONT_WIDTH;
        let y0 = self.window.y + row * FONT_HEIGHT;
        for dy in 0..FONT_HEIGHT {
            for dx in 0..FONT_WIDTH {
                put_pixel(self.buf, self.info, x0 + dx, y0 + dy, 0, 0, 0);
            }
        }
    }

    fn draw_cursor(&mut self) {
        if !self.cursor_visible {
            let x = self.window.x + self.col * FONT_WIDTH;
            let y0 = self.window.y + self.row * FONT_HEIGHT;
            for dy in 0..FONT_HEIGHT {
                put_pixel(self.buf, self.info, x, y0 + dy, 0xFF, 0xFF, 0xFF);
            }
            self.cursor_visible = true;
        }
    }

    fn erase_cursor(&mut self) {
        if self.cursor_visible {
            let x = self.window.x + self.col * FONT_WIDTH;
            let y0 = self.window.y + self.row * FONT_HEIGHT;
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
        let row_bytes = self.window.width * bpp;
        let window_y_end = self.window.y + self.window.height;

        // Copy window content up by one text row.
        let max_copy_rows = self.window.height.saturating_sub(FONT_HEIGHT);
        for wy in 0..max_copy_rows {
            let src_y = self.window.y + wy + FONT_HEIGHT;
            let dst_y = self.window.y + wy;
            if src_y + 1 > window_y_end {
                break;
            }
            let src_start = (src_y * self.info.stride + self.window.x) * bpp;
            let dst_start = (dst_y * self.info.stride + self.window.x) * bpp;
            if src_start + row_bytes <= self.buf.len() && dst_start + row_bytes <= self.buf.len() {
                self.buf
                    .copy_within(src_start..(src_start + row_bytes), dst_start);
            }
        }

        // Clear the last text row inside the window.
        let clear_y_start = window_y_end.saturating_sub(FONT_HEIGHT);
        fill_rect(
            self.buf,
            self.info,
            self.window.x,
            clear_y_start,
            self.window.width,
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

/// Initialize the framebuffer console. Call once from kernel_main.
pub fn init(buf: &'static mut [u8], info: FrameBufferInfo, window: Window) {
    fill_rect(buf, info, 0, 0, info.width, info.height, 0x30, 0x30, 0x30);
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
    *CONSOLE.lock() = Some(Console::new(buf, info, window));
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
