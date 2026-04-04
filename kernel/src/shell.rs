use crate::{keyboard, task, wm};
use pc_keyboard::DecodedKey;

const MAX_LINE: usize = 256;
const PROMPT: &str = "r-os> ";

/// Shell task entry point. Runs as an independent task with its own window.
pub fn task_main() -> ! {
    let win_id = task::current_window_id().unwrap_or(0);

    wm::console_write(
        win_id,
        "Welcome to r-os shell.\nType 'help' for commands.\n\n",
    );
    print_prompt(win_id);
    wm::console_show_cursor(win_id);

    let mut line_buf = [0u8; MAX_LINE];
    let mut line_len: usize = 0;

    loop {
        // Only process keyboard input when focused
        if wm::is_focused(win_id) {
            if let Some(key) = keyboard::try_read_key() {
                wm::console_hide_cursor(win_id);
                match key {
                    DecodedKey::Unicode(c) => {
                        handle_char(win_id, c, &mut line_buf, &mut line_len);
                    }
                    DecodedKey::RawKey(_) => {}
                }
                wm::console_show_cursor(win_id);
                wm::mark_dirty(win_id);
            }
        }

        task::yield_now();
    }
}

fn print_prompt(win_id: usize) {
    wm::console_write(win_id, PROMPT);
}

fn handle_char(win_id: usize, c: char, buf: &mut [u8; MAX_LINE], len: &mut usize) {
    match c {
        '\n' | '\r' => {
            wm::console_write(win_id, "\n");
            let cmd = core::str::from_utf8(&buf[..*len]).unwrap_or("");
            if !cmd.is_empty() {
                execute(win_id, cmd);
            }
            *len = 0;
            print_prompt(win_id);
        }
        '\u{8}' | '\u{7f}' => {
            if *len > 0 {
                *len -= 1;
                wm::console_write(win_id, "\u{8} \u{8}");
            }
        }
        c if c.is_ascii() && !c.is_ascii_control() => {
            if *len < MAX_LINE {
                buf[*len] = c as u8;
                *len += 1;
                let mut tmp = [0u8; 4];
                let s = c.encode_utf8(&mut tmp);
                wm::console_write(win_id, s);
            }
        }
        _ => {}
    }
}

fn execute(win_id: usize, cmd: &str) {
    let cmd = cmd.trim();
    let (command, args) = match cmd.find(' ') {
        Some(pos) => (&cmd[..pos], cmd[pos + 1..].trim()),
        None => (cmd, ""),
    };

    match command {
        "help" => cmd_help(win_id),
        "echo" => cmd_echo(win_id, args),
        "clear" => cmd_clear(win_id),
        "meminfo" => cmd_meminfo(win_id),
        "halt" => cmd_halt(),
        _ => {
            wm::console_write(win_id, "Unknown command: '");
            wm::console_write(win_id, command);
            wm::console_write(win_id, "'\n");
        }
    }
}

fn cmd_help(win_id: usize) {
    wm::console_write(win_id, "Available commands:\n");
    wm::console_write(win_id, "  help  - Show this help\n");
    wm::console_write(win_id, "  echo  - Print arguments\n");
    wm::console_write(win_id, "  clear - Clear screen\n");
    wm::console_write(win_id, "  meminfo - Heap info\n");
    wm::console_write(win_id, "  halt  - Halt CPU\n");
}

fn cmd_echo(win_id: usize, args: &str) {
    wm::console_write(win_id, args);
    wm::console_write(win_id, "\n");
}

fn cmd_clear(win_id: usize) {
    wm::console_clear(win_id);
}

fn cmd_meminfo(win_id: usize) {
    use crate::allocator;
    // Format heap start
    wm::console_write(win_id, "Heap: 0x4444_4444_0000\n");
    let size_kb = allocator::HEAP_SIZE / 1024;
    let mut buf = [0u8; 24];
    let len = format_u64(&mut buf, size_kb);
    wm::console_write(win_id, "Size: ");
    wm::console_write(win_id, core::str::from_utf8(&buf[..len]).unwrap_or("?"));
    wm::console_write(win_id, " KiB\n");
}

fn cmd_halt() -> ! {
    loop {
        x86_64::instructions::hlt();
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
