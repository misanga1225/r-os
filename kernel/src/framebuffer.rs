use bootloader_api::info::{FrameBufferInfo, PixelFormat};

/// フレームバッファ上の (x, y) に RGB 色のピクセルを描画する。
///
/// 範囲外の座標や未対応のピクセル形式は静かに無視される。
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
