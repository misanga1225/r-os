use crate::{interrupts, task, wm};

/// Counter task: displays a live tick counter to demonstrate multitasking.
pub fn task_main() -> ! {
    let win_id = task::current_window_id().unwrap_or(0);

    wm::console_write(win_id, "Tick Counter\n\n");
    wm::console_write(win_id, "This task runs in parallel\n");
    wm::console_write(win_id, "with the shell.\n\n");

    let mut last_displayed: u64 = 0;

    loop {
        let ticks = interrupts::ticks();

        // Update display every ~16 ticks (roughly 1/4 second at ~55Hz PIT)
        if ticks.wrapping_sub(last_displayed) >= 16 {
            last_displayed = ticks;

            // Clear and redraw the counter area
            wm::console_clear(win_id);
            wm::console_write(win_id, "Tick Counter\n\n");

            // Format tick count
            let mut buf = [0u8; 24];
            let len = format_u64(&mut buf, ticks);
            let s = core::str::from_utf8(&buf[..len]).unwrap_or("?");

            wm::console_write(win_id, "Ticks: ");
            wm::console_write(win_id, s);
            wm::console_write(win_id, "\n\n");

            // Also show uptime in seconds (PIT default ~18.2 Hz)
            let seconds = ticks / 18;
            let mut buf2 = [0u8; 24];
            let len2 = format_u64(&mut buf2, seconds);
            let s2 = core::str::from_utf8(&buf2[..len2]).unwrap_or("?");
            wm::console_write(win_id, "Uptime: ");
            wm::console_write(win_id, s2);
            wm::console_write(win_id, "s\n");

            wm::mark_dirty(win_id);
        }

        task::yield_now();
    }
}

fn format_u64(buf: &mut [u8; 24], val: u64) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut digits = [0u8; 20];
    let mut n = val;
    let mut dlen = 0;
    while n > 0 {
        digits[dlen] = (n % 10) as u8 + b'0';
        n /= 10;
        dlen += 1;
    }
    for (i, d) in (0..dlen).rev().enumerate() {
        buf[i] = digits[d];
    }
    dlen
}
