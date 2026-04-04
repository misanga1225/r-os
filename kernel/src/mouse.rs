use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use x86_64::instructions::port::Port;

// ---------------------------------------------------------------------------
// MouseEvent
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct MouseEvent {
    pub dx: i16,
    pub dy: i16,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

// ---------------------------------------------------------------------------
// Lock-free SPSC Ring Buffer
// ---------------------------------------------------------------------------
//
// 単一プロデューサ（マウスISR）と単一コンシューマ（メインループ）を前提とした
// ロックフリーリングバッファ。keyboard.rs と同じパターン。

const BUF_SIZE: usize = 128;

struct RingBuffer {
    data: UnsafeCell<[MouseEvent; BUF_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

/// Safety: 単一プロデューサ（ISR）・単一コンシューマ（メインループ）の不変条件により安全。
unsafe impl Sync for RingBuffer {}

const EMPTY_EVENT: MouseEvent = MouseEvent {
    dx: 0,
    dy: 0,
    left: false,
    right: false,
    middle: false,
};

static MOUSE_QUEUE: RingBuffer = RingBuffer {
    data: UnsafeCell::new([EMPTY_EVENT; BUF_SIZE]),
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
};

impl RingBuffer {
    /// # Safety
    /// 単一プロデューサ（マウスISR）からのみ呼び出すこと。
    unsafe fn push(&self, val: MouseEvent) {
        let head = self.head.load(Ordering::Relaxed);
        let next = (head + 1) % BUF_SIZE;
        if next == self.tail.load(Ordering::Acquire) {
            return; // バッファ満杯 — 破棄
        }
        unsafe {
            (*self.data.get())[head] = val;
        }
        self.head.store(next, Ordering::Release);
    }

    fn pop(&self) -> Option<MouseEvent> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        let val = unsafe { (*self.data.get())[tail] };
        self.tail.store((tail + 1) % BUF_SIZE, Ordering::Release);
        Some(val)
    }
}

// ---------------------------------------------------------------------------
// PS/2 Packet State Machine
// ---------------------------------------------------------------------------
//
// PS/2マウスは3バイトパケットを送信する:
//   byte0: [y_overflow, x_overflow, y_sign, x_sign, always_1, middle, right, left]
//   byte1: X movement (符号なし、符号はbyte0のbit4)
//   byte2: Y movement (符号なし、符号はbyte0のbit5)
//
// ISRから1バイトずつ呼ばれ、3バイト揃ったらデコードしてキューに追加する。

// 0 = Byte0待ち, 1 = Byte1待ち, 2 = Byte2待ち
static PACKET_STATE: AtomicU8 = AtomicU8::new(0);

// ISRからのみアクセスされる（単一プロデューサ）
struct SyncUnsafeCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

static PACKET_BYTES: SyncUnsafeCell<[u8; 3]> = SyncUnsafeCell(UnsafeCell::new([0; 3]));

/// マウスISRから呼び出される。1バイトずつパケットを組み立て、
/// 3バイト揃ったらデコードしてリングバッファに追加する。
///
/// # Safety
/// マウス割り込みハンドラ（単一プロデューサ）からのみ呼び出すこと。
pub fn add_byte(byte: u8) {
    let state = PACKET_STATE.load(Ordering::Relaxed);

    match state {
        0 => {
            // Byte0: bit3 (always-1) が立っていなければ同期ズレ — 破棄してリトライ
            if byte & 0x08 == 0 {
                return;
            }
            unsafe { (*PACKET_BYTES.0.get())[0] = byte };
            PACKET_STATE.store(1, Ordering::Relaxed);
        }
        1 => {
            unsafe { (*PACKET_BYTES.0.get())[1] = byte };
            PACKET_STATE.store(2, Ordering::Relaxed);
        }
        2 => {
            unsafe { (*PACKET_BYTES.0.get())[2] = byte };
            PACKET_STATE.store(0, Ordering::Relaxed);

            let bytes = unsafe { *PACKET_BYTES.0.get() };
            let b0 = bytes[0];

            // オーバーフロービット（bit6, bit7）が立っていたらパケットを破棄
            if b0 & 0xC0 != 0 {
                return;
            }

            let left = b0 & 0x01 != 0;
            let right = b0 & 0x02 != 0;
            let middle = b0 & 0x04 != 0;

            // 符号拡張
            let dx = if b0 & 0x10 != 0 {
                bytes[1] as i16 - 256
            } else {
                bytes[1] as i16
            };
            let dy = if b0 & 0x20 != 0 {
                bytes[2] as i16 - 256
            } else {
                bytes[2] as i16
            };

            let event = MouseEvent {
                dx,
                dy,
                left,
                right,
                middle,
            };

            unsafe {
                MOUSE_QUEUE.push(event);
            }
        }
        _ => {
            // 不正な状態 — リセット
            PACKET_STATE.store(0, Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// デコード済みマウスイベントを1つ取得する。イベントがなければ None を返す。
/// メインループからのみ呼び出すこと。
pub fn try_read_event() -> Option<MouseEvent> {
    MOUSE_QUEUE.pop()
}

// ---------------------------------------------------------------------------
// PS/2 Mouse Initialization
// ---------------------------------------------------------------------------

const STATUS_PORT: u16 = 0x64;
const DATA_PORT: u16 = 0x60;
const TIMEOUT: u32 = 100_000;

fn wait_for_write() {
    let mut port: Port<u8> = Port::new(STATUS_PORT);
    for _ in 0..TIMEOUT {
        let status = unsafe { port.read() };
        if status & 0x02 == 0 {
            return;
        }
    }
}

fn wait_for_read() {
    let mut port: Port<u8> = Port::new(STATUS_PORT);
    for _ in 0..TIMEOUT {
        let status = unsafe { port.read() };
        if status & 0x01 != 0 {
            return;
        }
    }
}

fn write_command(cmd: u8) {
    wait_for_write();
    let mut port: Port<u8> = Port::new(STATUS_PORT);
    unsafe { port.write(cmd) };
}

fn write_data(data: u8) {
    wait_for_write();
    let mut port: Port<u8> = Port::new(DATA_PORT);
    unsafe { port.write(data) };
}

fn read_data() -> u8 {
    wait_for_read();
    let mut port: Port<u8> = Port::new(DATA_PORT);
    unsafe { port.read() }
}

fn flush_output_buffer() {
    let mut status_port: Port<u8> = Port::new(STATUS_PORT);
    let mut data_port: Port<u8> = Port::new(DATA_PORT);
    for _ in 0..TIMEOUT {
        let status = unsafe { status_port.read() };
        if status & 0x01 == 0 {
            break;
        }
        unsafe { data_port.read() }; // 読み捨て
    }
}

fn write_mouse(byte: u8) {
    write_command(0xD4); // 次のバイトをマウスに転送
    write_data(byte);
}

/// PS/2マウスを初期化する。
/// PIC初期化後、割り込み有効化前に呼び出すこと。
pub fn init() {
    // 1. キーボード・マウスポートを一時無効化
    write_command(0xAD); // キーボード無効化
    write_command(0xA7); // マウス無効化

    // 2. 出力バッファをフラッシュ
    flush_output_buffer();

    // 3. マウスポート有効化
    write_command(0xA8);

    // 4. コントローラ設定バイトを読み出し、マウスIRQ(bit1)を有効化
    write_command(0x20); // 設定バイト読み出し要求
    let config = read_data();
    let new_config = config | 0x02; // bit1: マウス割り込み有効
    write_command(0x60); // 設定バイト書き込み要求
    write_data(new_config);

    // 5. マウスにデフォルト設定を送信
    write_mouse(0xF6); // Set Defaults
    let _ack = read_data(); // ACK (0xFA)

    // 6. マウスにストリーミングモードを有効化
    write_mouse(0xF4); // Enable Data Reporting
    let _ack = read_data(); // ACK (0xFA)

    // 7. キーボードポート再有効化
    write_command(0xAE);
}
