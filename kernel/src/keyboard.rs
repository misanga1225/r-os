use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts};
use spin::{Lazy, Mutex};

// ---------------------------------------------------------------------------
// Lock-free SPSC Ring Buffer
// ---------------------------------------------------------------------------
//
// 単一プロデューサ（キーボードISR）と単一コンシューマ（メインループ）を前提とした
// ロックフリーリングバッファ。ISR内でロックを取得する必要がない。

const BUF_SIZE: usize = 128;

struct RingBuffer {
    data: UnsafeCell<[u8; BUF_SIZE]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

/// Safety: 単一プロデューサ（ISR）・単一コンシューマ（メインループ）の不変条件により安全。
/// head は ISR のみが書き込み、tail はメインループのみが書き込む。
unsafe impl Sync for RingBuffer {}

static SCANCODE_QUEUE: RingBuffer = RingBuffer {
    data: UnsafeCell::new([0; BUF_SIZE]),
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
};

impl RingBuffer {
    /// スキャンコードをバッファに追加する。バッファが満杯の場合は破棄する。
    ///
    /// # Safety
    /// 単一プロデューサ（キーボードISR）からのみ呼び出すこと。
    unsafe fn push(&self, val: u8) {
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

    /// スキャンコードをバッファから取り出す。空の場合は None を返す。
    ///
    /// 単一コンシューマ（メインループ）からのみ呼び出すこと。
    fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None; // 空
        }
        let val = unsafe { (*self.data.get())[tail] };
        self.tail.store((tail + 1) % BUF_SIZE, Ordering::Release);
        Some(val)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// キーボードISRから呼び出される。スキャンコードをリングバッファに追加する。
///
/// # Safety
/// キーボード割り込みハンドラ（単一プロデューサ）からのみ呼び出すこと。
pub fn add_scancode(scancode: u8) {
    unsafe {
        SCANCODE_QUEUE.push(scancode);
    }
}

static KEYBOARD: Lazy<Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>>> = Lazy::new(|| {
    Mutex::new(Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    ))
});

/// デコード済みキー入力を1つ取得する。入力がなければ None を返す。
/// メインループからのみ呼び出すこと。
pub fn try_read_key() -> Option<DecodedKey> {
    loop {
        let scancode = SCANCODE_QUEUE.pop()?;
        let mut kb = KEYBOARD.lock();
        if let Ok(Some(event)) = kb.add_byte(scancode) {
            if let Some(key) = kb.process_keyevent(event) {
                return Some(key);
            }
        }
        // スキャンコードがキーイベントに変換されなかった場合（例: キーリリース）、
        // バッファに残りがあれば次のスキャンコードを試行する
    }
}
