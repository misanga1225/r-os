extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use bootloader_api::info::FrameBufferInfo;
use spin::Mutex;

use crate::framebuffer::{
    self, CURSOR_H, CURSOR_SPRITE, CURSOR_W, Console, DEFAULT_TITLE_BAR_HEIGHT, FONT_HEIGHT,
    FONT_WIDTH, Rect, WINDOW_PADDING,
};
use crate::mouse::MouseEvent;

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

const MAX_TITLE: usize = 32;

pub struct WmWindow {
    pub id: usize,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title: [u8; MAX_TITLE],
    pub title_len: usize,
    pub buf: Vec<u8>,
    pub buf_info: FrameBufferInfo,
    pub console: Option<Console>,
    pub dirty: bool,
    pub owner_task: usize,
}

impl WmWindow {
    fn title_str(&self) -> &str {
        core::str::from_utf8(&self.title[..self.title_len]).unwrap_or("?")
    }

    /// The content rect within the window's own buffer (relative to buf origin 0,0)
    pub fn content_rect(&self) -> Rect {
        let title_h = DEFAULT_TITLE_BAR_HEIGHT.min(self.height);
        Rect {
            x: WINDOW_PADDING,
            y: title_h + WINDOW_PADDING,
            width: self.width.saturating_sub(WINDOW_PADDING * 2),
            height: self.height.saturating_sub(title_h + WINDOW_PADDING * 2),
        }
    }

    /// Redraw only the window chrome (title bar + border). Content area is preserved.
    fn draw_chrome(&mut self, focused: bool) {
        let info = self.buf_info;
        let w = self.width;
        let h = self.height;
        let title_len = self.title_len;
        let title_copy: [u8; MAX_TITLE] = self.title;

        let buf = &mut self.buf;

        // Title bar only (not the content area)
        let title_h = DEFAULT_TITLE_BAR_HEIGHT.min(h);
        let tb_color = if focused {
            (0x20, 0x3A, 0x58)
        } else {
            (0x18, 0x2A, 0x40)
        };
        framebuffer::fill_rect(
            buf, info, 0, 0, w, title_h, tb_color.0, tb_color.1, tb_color.2,
        );

        // Border
        framebuffer::draw_rect_outline(
            buf,
            info,
            Rect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            },
            0x55,
            0x5A,
            0x66,
        );

        // Title text
        if title_h >= FONT_HEIGHT {
            let text_x = WINDOW_PADDING;
            let text_y = (title_h - FONT_HEIGHT) / 2;
            let title_str = core::str::from_utf8(&title_copy[..title_len]).unwrap_or("?");
            framebuffer::draw_text(buf, info, text_x, text_y, title_str, 0xF0, 0xF2, 0xF6);
        }
    }

    /// Initialize window: fill black + draw chrome. Called once at creation.
    fn init_chrome(&mut self) {
        let info = self.buf_info;
        framebuffer::fill_rect(&mut self.buf, info, 0, 0, self.width, self.height, 0, 0, 0);
        self.draw_chrome(false);
    }
}

// ---------------------------------------------------------------------------
// Mouse cursor state (managed by WM)
// ---------------------------------------------------------------------------

struct CursorState {
    x: i32,
    y: i32,
    screen_width: i32,
    screen_height: i32,
    saved_bg: [[u8; 3]; CURSOR_W * CURSOR_H],
    visible: bool,
}

impl CursorState {
    fn new(w: usize, h: usize) -> Self {
        CursorState {
            x: (w / 2) as i32,
            y: (h / 2) as i32,
            screen_width: w as i32,
            screen_height: h as i32,
            saved_bg: [[0; 3]; CURSOR_W * CURSOR_H],
            visible: false,
        }
    }

    fn save_background(&mut self, buf: &[u8], info: FrameBufferInfo) {
        let cx = self.x as usize;
        let cy = self.y as usize;
        for sy in 0..CURSOR_H {
            for sx in 0..CURSOR_W {
                let (r, g, b) = framebuffer::read_pixel(buf, info, cx + sx, cy + sy);
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
                    framebuffer::put_pixel(buf, info, cx + sx, cy + sy, r, g, b);
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
                    1 => framebuffer::put_pixel(buf, info, cx + sx, cy + sy, 0x00, 0x00, 0x00),
                    2 => framebuffer::put_pixel(buf, info, cx + sx, cy + sy, 0xFF, 0xFF, 0xFF),
                    _ => {}
                }
            }
        }
    }

    fn apply_delta(&mut self, dx: i16, dy: i16) {
        self.x += dx as i32;
        self.y -= dy as i32; // PS/2 Y-axis inverted
        self.x = self.x.clamp(0, self.screen_width - 1);
        self.y = self.y.clamp(0, self.screen_height - 1);
    }
}

// ---------------------------------------------------------------------------
// Window Manager
// ---------------------------------------------------------------------------

pub struct WindowManager {
    windows: Vec<WmWindow>,
    focused: Option<usize>, // window id
    next_id: usize,
    cursor: CursorState,
    screen_info: FrameBufferInfo,
    taskbar_height: usize,
    needs_composite: bool,
    back_buf: Vec<u8>, // screen-sized back buffer (double buffering)
    bg_cache: Vec<u8>, // cached desktop background
}

const TASKBAR_HEIGHT: usize = 28;

impl WindowManager {
    fn new(screen_info: FrameBufferInfo) -> Self {
        let buf_size = screen_info.width * screen_info.height * screen_info.bytes_per_pixel;

        // Pre-render background to cache
        let mut bg_cache = vec![0u8; buf_size];
        framebuffer::draw_background(&mut bg_cache, screen_info);

        WindowManager {
            windows: Vec::new(),
            focused: None,
            next_id: 0,
            cursor: CursorState::new(screen_info.width, screen_info.height),
            screen_info,
            taskbar_height: TASKBAR_HEIGHT,
            needs_composite: true,
            back_buf: vec![0u8; buf_size],
            bg_cache,
        }
    }

    pub fn create_window(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        title: &str,
        owner_task: usize,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        // Create per-window buffer with same pixel format as screen
        let mut buf_info = self.screen_info;
        buf_info.width = width;
        buf_info.height = height;
        buf_info.stride = width;

        let buf_size = width * height * buf_info.bytes_per_pixel;
        let buf = vec![0u8; buf_size];

        let mut title_buf = [0u8; MAX_TITLE];
        let tlen = title.len().min(MAX_TITLE);
        title_buf[..tlen].copy_from_slice(&title.as_bytes()[..tlen]);

        let mut win = WmWindow {
            id,
            x,
            y,
            width,
            height,
            title: title_buf,
            title_len: tlen,
            buf,
            buf_info,
            console: None,
            dirty: true,
            owner_task,
        };

        let focused = self.focused.is_none() || self.focused == Some(id);
        win.draw_chrome(focused);

        self.windows.push(win);

        if self.focused.is_none() {
            self.focused = Some(id);
        }

        self.needs_composite = true;
        id
    }

    pub fn init_console(&mut self, win_id: usize) {
        if let Some(win) = self.window_mut(win_id) {
            let content = win.content_rect();
            let console = Console::new(&mut win.buf, win.buf_info, content);
            win.console = Some(console);
        }
    }

    pub fn window_mut(&mut self, id: usize) -> Option<&mut WmWindow> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn window(&self, id: usize) -> Option<&WmWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn is_focused(&self, id: usize) -> bool {
        self.focused == Some(id)
    }

    pub fn mark_dirty(&mut self, id: usize) {
        if let Some(win) = self.window_mut(id) {
            win.dirty = true;
        }
        self.needs_composite = true;
    }

    pub fn focus_window(&mut self, id: usize) {
        if self.focused == Some(id) {
            return;
        }

        // Redraw old focused window's chrome as unfocused
        if let Some(old_id) = self.focused {
            if let Some(win) = self.windows.iter_mut().find(|w| w.id == old_id) {
                win.draw_chrome(false);
                // Redraw console content (chrome overwrites content area)
                // Console content is preserved in the buffer, only title bar changes
                win.dirty = true;
            }
        }

        // Bring target window to front (move to end of vec)
        if let Some(pos) = self.windows.iter().position(|w| w.id == id) {
            let win = self.windows.remove(pos);
            self.windows.push(win);
        }

        // Draw new focused window's chrome
        if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
            win.draw_chrome(true);
            win.dirty = true;
        }

        self.focused = Some(id);
        self.needs_composite = true;
    }

    /// Composite all windows onto the back buffer, then flip to framebuffer.
    pub fn composite(&mut self, fb_buf: &mut [u8], fb_info: FrameBufferInfo) {
        // 1. Copy cached background to back buffer (fast memcpy)
        self.back_buf.copy_from_slice(&self.bg_cache);

        // 2. Draw taskbar on back buffer
        draw_taskbar(
            &mut self.back_buf,
            fb_info,
            self.taskbar_height,
            &self.windows,
            self.focused,
        );

        // 3. Blit each window (back to front) on back buffer
        for win in &self.windows {
            blit_window(&mut self.back_buf, fb_info, win);
        }

        // 4. Draw mouse cursor on back buffer
        self.cursor.save_background(&self.back_buf, fb_info);
        self.cursor.draw(&mut self.back_buf, fb_info);
        self.cursor.visible = true;

        // 5. Flip: copy entire back buffer to framebuffer in one go
        fb_buf.copy_from_slice(&self.back_buf);

        self.needs_composite = false;
        for win in &mut self.windows {
            win.dirty = false;
        }
    }

    /// Handle a mouse event. Updates cursor position and focus.
    /// Returns true if only cursor moved (no focus change), false if full composite needed.
    pub fn handle_mouse(&mut self, event: MouseEvent, fb_buf: &mut [u8], fb_info: FrameBufferInfo) {
        // Restore old cursor from back buffer
        if self.cursor.visible {
            let cx = self.cursor.x as usize;
            let cy = self.cursor.y as usize;
            for sy in 0..CURSOR_H {
                for sx in 0..CURSOR_W {
                    if CURSOR_SPRITE[sy][sx] != 0 {
                        let [r, g, b] = self.cursor.saved_bg[sy * CURSOR_W + sx];
                        framebuffer::put_pixel(fb_buf, fb_info, cx + sx, cy + sy, r, g, b);
                    }
                }
            }
        }

        self.cursor.apply_delta(event.dx, event.dy);

        // Left click → focus window
        if event.left {
            let mx = self.cursor.x as usize;
            let my = self.cursor.y as usize;

            let tb_y = self.screen_info.height.saturating_sub(self.taskbar_height);
            if my >= tb_y {
                self.handle_taskbar_click(mx);
                self.needs_composite = true;
            } else {
                let mut clicked_id = None;
                for win in self.windows.iter().rev() {
                    if mx >= win.x
                        && mx < win.x + win.width
                        && my >= win.y
                        && my < win.y + win.height
                    {
                        clicked_id = Some(win.id);
                        break;
                    }
                }
                if let Some(id) = clicked_id {
                    self.focus_window(id);
                    // needs_composite already set by focus_window
                }
            }
        }

        if !self.needs_composite {
            // Just redraw cursor at new position directly on framebuffer
            self.cursor.save_background(fb_buf, fb_info);
            self.cursor.draw(fb_buf, fb_info);
            self.cursor.visible = true;
        }
        // If needs_composite, the full composite will handle cursor drawing
    }

    fn handle_taskbar_click(&mut self, mx: usize) {
        let sep_x = 8 + 4 * FONT_WIDTH + 6 + 8;
        let mut lx = sep_x;
        // Find which window label was clicked
        let mut target = None;
        for win in &self.windows {
            let label_w = win.title_len * FONT_WIDTH + 16;
            if mx >= lx.saturating_sub(4) && mx < lx + label_w {
                target = Some(win.id);
                break;
            }
            lx += label_w;
        }
        if let Some(id) = target {
            self.focus_window(id);
        }
    }

    pub fn needs_composite(&self) -> bool {
        self.needs_composite
    }

    pub fn request_composite(&mut self) {
        self.needs_composite = true;
    }
} // impl WindowManager

fn draw_taskbar(
    buf: &mut [u8],
    info: FrameBufferInfo,
    taskbar_height: usize,
    windows: &[WmWindow],
    focused: Option<usize>,
) {
    let tb_y = info.height.saturating_sub(taskbar_height);

    framebuffer::fill_rect(
        buf,
        info,
        0,
        tb_y,
        info.width,
        taskbar_height,
        0x1A,
        0x1E,
        0x28,
    );
    framebuffer::fill_rect(buf, info, 0, tb_y, info.width, 1, 0x33, 0x38, 0x44);
    framebuffer::draw_text(buf, info, 8, tb_y + 6, "r-os", 0xE0, 0xE4, 0xEC);

    let sep_x = 8 + 4 * FONT_WIDTH + 6;
    framebuffer::fill_rect(
        buf,
        info,
        sep_x,
        tb_y + 4,
        1,
        taskbar_height - 8,
        0x44,
        0x48,
        0x55,
    );

    let mut lx = sep_x + 8;
    for win in windows {
        let is_focused = focused == Some(win.id);
        let (tr, tg, tb_c) = if is_focused {
            (0xFF, 0xFF, 0xFF)
        } else {
            (0x88, 0x8C, 0x96)
        };

        if is_focused {
            let label_w = win.title_len * FONT_WIDTH + 12;
            framebuffer::fill_rect(
                buf,
                info,
                lx - 4,
                tb_y + 2,
                label_w,
                taskbar_height - 4,
                0x28,
                0x32,
                0x44,
            );
        }

        framebuffer::draw_text(buf, info, lx, tb_y + 6, win.title_str(), tr, tg, tb_c);
        lx += win.title_len * FONT_WIDTH + 16;
    }
}

/// Blit a window's buffer onto the main framebuffer at (win.x, win.y)
fn blit_window(fb_buf: &mut [u8], fb_info: FrameBufferInfo, win: &WmWindow) {
    let bpp = fb_info.bytes_per_pixel;

    // Clip to screen bounds
    let src_x_start = 0usize;
    let src_y_start = 0usize;
    let dst_x = win.x;
    let dst_y = win.y;
    let copy_w = win.width.min(fb_info.width.saturating_sub(dst_x));
    let copy_h = win.height.min(fb_info.height.saturating_sub(dst_y));

    for row in 0..copy_h {
        let src_offset = ((src_y_start + row) * win.buf_info.stride + src_x_start) * bpp;
        let dst_offset = ((dst_y + row) * fb_info.stride + dst_x) * bpp;
        let bytes = copy_w * bpp;

        if src_offset + bytes <= win.buf.len() && dst_offset + bytes <= fb_buf.len() {
            fb_buf[dst_offset..dst_offset + bytes]
                .copy_from_slice(&win.buf[src_offset..src_offset + bytes]);
        }
    }
}

// ---------------------------------------------------------------------------
// Global WM singleton
// ---------------------------------------------------------------------------

static WM: Mutex<Option<WindowManager>> = Mutex::new(None);

pub fn init() {
    if let Some(info) = framebuffer::info() {
        *WM.lock() = Some(WindowManager::new(info));
    }
}

pub fn with_wm<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut WindowManager) -> R,
{
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(wm) = WM.lock().as_mut() {
            Some(f(wm))
        } else {
            None
        }
    })
}

/// Create a window. Returns window ID.
pub fn create_window(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    title: &str,
    owner_task: usize,
) -> usize {
    with_wm(|wm| wm.create_window(x, y, width, height, title, owner_task)).unwrap_or(0)
}

pub fn init_console(win_id: usize) {
    with_wm(|wm| wm.init_console(win_id));
}

pub fn is_focused(win_id: usize) -> bool {
    with_wm(|wm| wm.is_focused(win_id)).unwrap_or(false)
}

pub fn mark_dirty(win_id: usize) {
    with_wm(|wm| wm.mark_dirty(win_id));
}

/// Write a string to a window's console
pub fn console_write(win_id: usize, s: &str) {
    with_wm(|wm| {
        if let Some(win) = wm.window_mut(win_id) {
            if let Some(console) = win.console.as_mut() {
                let buf = &mut win.buf;
                let info = win.buf_info;
                console.write_str(buf, info, s);
                win.dirty = true;
            }
        }
        wm.needs_composite = true;
    });
}

pub fn console_show_cursor(win_id: usize) {
    with_wm(|wm| {
        if let Some(win) = wm.window_mut(win_id) {
            if let Some(console) = win.console.as_mut() {
                console.draw_cursor(&mut win.buf, win.buf_info);
                win.dirty = true;
            }
        }
        wm.needs_composite = true;
    });
}

pub fn console_hide_cursor(win_id: usize) {
    with_wm(|wm| {
        if let Some(win) = wm.window_mut(win_id) {
            if let Some(console) = win.console.as_mut() {
                console.erase_cursor(&mut win.buf, win.buf_info);
                win.dirty = true;
            }
        }
        wm.needs_composite = true;
    });
}

pub fn console_clear(win_id: usize) {
    with_wm(|wm| {
        if let Some(win) = wm.window_mut(win_id) {
            if let Some(console) = win.console.as_mut() {
                console.clear(&mut win.buf, win.buf_info);
                win.dirty = true;
            }
        }
        wm.needs_composite = true;
    });
}

/// Handle mouse event. Updates cursor and focus. Only does full composite if needed.
pub fn handle_mouse(event: MouseEvent) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut wm_guard = WM.lock();
        let mut fb_guard = crate::framebuffer::FB_STATE.lock();
        if let (Some(wm), Some(fb)) = (wm_guard.as_mut(), fb_guard.as_mut()) {
            wm.handle_mouse(event, fb.buf, fb.info);
            if wm.needs_composite {
                wm.composite(fb.buf, fb.info);
            }
        }
    });
}

/// Composite all windows to framebuffer.
pub fn composite() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut wm_guard = WM.lock();
        let mut fb_guard = crate::framebuffer::FB_STATE.lock();
        if let (Some(wm), Some(fb)) = (wm_guard.as_mut(), fb_guard.as_mut()) {
            wm.composite(fb.buf, fb.info);
        }
    });
}
