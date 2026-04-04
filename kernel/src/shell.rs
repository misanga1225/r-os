use crate::{keyboard, mouse};
use crate::{print, println};
use pc_keyboard::DecodedKey;

const MAX_LINE: usize = 256;
const PROMPT: &str = "r-os> ";

/// シェルのメインループ。kernel_main の初期化完了後に呼び出す。
/// この関数は返らない。
pub fn run() -> ! {
    let mut line_buf = [0u8; MAX_LINE];
    let mut line_len: usize = 0;

    print_prompt();
    crate::framebuffer::show_cursor();

    loop {
        let mut had_input = false;

        if let Some(key) = keyboard::try_read_key() {
            had_input = true;
            crate::framebuffer::hide_cursor();
            match key {
                DecodedKey::Unicode(c) => {
                    handle_char(c, &mut line_buf, &mut line_len);
                }
                DecodedKey::RawKey(_) => {
                    // 特殊キー（矢印・Fキー等）は現時点では無視
                }
            }
            crate::framebuffer::show_cursor();
        }

        while let Some(event) = mouse::try_read_event() {
            had_input = true;
            crate::framebuffer::update_cursor(event);
        }

        if !had_input {
            x86_64::instructions::hlt();
        }
    }
}

fn print_prompt() {
    print!("{}", PROMPT);
}

fn handle_char(c: char, buf: &mut [u8; MAX_LINE], len: &mut usize) {
    match c {
        '\n' | '\r' => {
            println!();
            let cmd = core::str::from_utf8(&buf[..*len]).unwrap_or("");
            if !cmd.is_empty() {
                execute(cmd);
            }
            *len = 0;
            print_prompt();
        }
        '\u{8}' | '\u{7f}' => {
            // Backspace / Delete
            if *len > 0 {
                *len -= 1;
                // ターミナル上で1文字消去: backspace → space → backspace
                print!("\u{8} \u{8}");
            }
        }
        c if c.is_ascii() && !c.is_ascii_control() => {
            if *len < MAX_LINE {
                buf[*len] = c as u8;
                *len += 1;
                print!("{}", c);
            }
        }
        _ => {} // 非ASCII・その他制御文字は無視
    }
}

fn execute(cmd: &str) {
    let cmd = cmd.trim();
    let (command, args) = match cmd.find(' ') {
        Some(pos) => (&cmd[..pos], cmd[pos + 1..].trim()),
        None => (cmd, ""),
    };

    match command {
        "help" => cmd_help(),
        "echo" => cmd_echo(args),
        "clear" => cmd_clear(),
        "meminfo" => cmd_meminfo(),
        "halt" => cmd_halt(),
        _ => println!(
            "Unknown command: '{}'. Type 'help' for available commands.",
            command
        ),
    }
}

fn cmd_help() {
    println!("Available commands:");
    println!("  help     - Show this help message");
    println!("  echo     - Print arguments to output");
    println!("  clear    - Clear the terminal screen");
    println!("  meminfo  - Show heap memory info");
    println!("  halt     - Halt the CPU");
}

fn cmd_echo(args: &str) {
    println!("{}", args);
}

fn cmd_clear() {
    // フレームバッファをクリア + シリアル側も ANSI エスケープでクリア
    crate::framebuffer::clear();
    crate::serial::_print(format_args!("\x1B[2J\x1B[H"));
}

fn cmd_meminfo() {
    println!("Heap start:  {:#x}", crate::allocator::HEAP_START);
    println!("Heap size:   {} KiB", crate::allocator::HEAP_SIZE / 1024);
}

fn cmd_halt() -> ! {
    println!("Halting CPU.");
    loop {
        x86_64::instructions::hlt();
    }
}
