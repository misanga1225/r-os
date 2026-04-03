# r-os
Rust製の自作OS（一応）．ブートローダは `bootloader` を使用し，HALに注力．

## 進捗管理

- 描画実装
  - UART(シリアルポート0x3F8)への出力を実装(メモリマップ表示)
  - VGAへの描画を実装
  - QEMUウィンドウ表示を有効にし，画面描画後は `hlt` ループ待機
- メモリ管理実装
  - フレームアロケータ: Usableリージョンから4KiB物理フレームを払い出す
  - ページテーブル: 任意の仮想ページを物理フレームにマッピング可能に
    - x86_64アーキテクチャのカノニカルアドレス制約の回避
  - ヒープアロケータ: `Box`, `Vec`, `String` 等が使用可能に
- 割り込み実装
  - GDT/TSS: ダブルフォールト用の専用スタック(IST)を設定
  - IDT: CPU例外ハンドラを登録し `lidt` でCPUにロード
    - Divide Error, Breakpoint, Invalid Opcode, Double Fault, General Protection Fault, Page Fault
  - PIC (8259): タイマー，キーボード割り込み実装
    - キーボード: スキャンコードをシリアルに出力
  - デッドロック対策: シリアル出力中は `without_interrupts` で割り込みを抑制
