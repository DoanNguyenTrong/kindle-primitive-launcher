//! Gray images for app icons: PNG decoding, scaling into the icon box, dithering to the panel's 16 gray
//! steps, and a small cache file format so icons are not decoded on every start.

use std::io::Cursor;

/// An 8-bit gray image, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrayImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl GrayImage {
    pub fn blank(width: u32, height: u32) -> GrayImage {
        GrayImage { width, height, pixels: vec![255; (width * height) as usize] }
    }
}

/// Largest PNG we decode (pixels). The Kindle's library icons are about 1000 × 1500.
const MAX_PIXELS: u64 = 4_000_000;

/// Decode a PNG into gray, compositing any transparency over white.
pub fn decode_png(bytes: &[u8]) -> Result<GrayImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
        return Err(format!("png: size {w}x{h} not supported"));
    }
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
    let data = &buf[..frame.buffer_size()];
    let channels = match frame.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("png: palette not expanded".into()),
    };
    let stride = frame.line_size;
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        let row = &data[y * stride..];
        for x in 0..w as usize {
            let p = &row[x * channels..x * channels + channels];
            let (luma, alpha) = match channels {
                1 => (p[0] as u32, 255),
                2 => (p[0] as u32, p[1] as u32),
                3 => (luma(p[0], p[1], p[2]), 255),
                _ => (luma(p[0], p[1], p[2]), p[3] as u32),
            };
            pixels.push(((luma * alpha + 255 * (255 - alpha)) / 255) as u8);
        }
    }
    Ok(GrayImage { width: w, height: h, pixels })
}

fn luma(r: u8, g: u8, b: u8) -> u32 {
    (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000
}

/// Scale `img` to fit inside `w` × `h` keeping its shape, centred on white. Each output pixel is the
/// average of the source area it covers, so large icons shrink without sparkle.
pub fn fit(img: &GrayImage, w: u32, h: u32) -> GrayImage {
    let mut out = GrayImage::blank(w, h);
    if img.width == 0 || img.height == 0 || w == 0 || h == 0 {
        return out;
    }
    let scale = (w as f64 / img.width as f64).min(h as f64 / img.height as f64);
    let (dw, dh) = (((img.width as f64 * scale).round() as u32).clamp(1, w), ((img.height as f64 * scale).round() as u32).clamp(1, h));
    let (ox, oy) = ((w - dw) / 2, (h - dh) / 2);
    for dy in 0..dh {
        let sy0 = dy as f64 / scale;
        let sy1 = ((dy + 1) as f64 / scale).min(img.height as f64);
        for dx in 0..dw {
            let sx0 = dx as f64 / scale;
            let sx1 = ((dx + 1) as f64 / scale).min(img.width as f64);
            out.pixels[((oy + dy) * w + ox + dx) as usize] = area_average(img, sx0, sy0, sx1, sy1);
        }
    }
    out
}

fn area_average(img: &GrayImage, x0: f64, y0: f64, x1: f64, y1: f64) -> u8 {
    let (ix0, iy0) = (x0.floor() as u32, y0.floor() as u32);
    let (ix1, iy1) = ((x1.ceil() as u32).min(img.width).max(ix0 + 1), (y1.ceil() as u32).min(img.height).max(iy0 + 1));
    let (mut sum, mut weight) = (0.0, 0.0);
    for y in iy0..iy1 {
        let wy = (((y + 1) as f64).min(y1) - (y as f64).max(y0)).max(0.0);
        for x in ix0..ix1 {
            let wx = (((x + 1) as f64).min(x1) - (x as f64).max(x0)).max(0.0);
            let wgt = wx * wy;
            sum += img.pixels[(y * img.width + x) as usize] as f64 * wgt;
            weight += wgt;
        }
    }
    if weight <= 0.0 {
        255
    } else {
        (sum / weight).round().clamp(0.0, 255.0) as u8
    }
}

/// The 8x8 ordered-dither thresholds (0..63), the Bayer matrix. From Primitive Browser.
const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// Ordered dither to the panel's 16 gray steps (multiples of 17), in place. The average gray of every
/// area is kept; flat areas get a fixed dot pattern, which stays calm on e-ink. From Primitive Browser.
pub fn dither16(img: &mut GrayImage) {
    let w = img.width as usize;
    for (i, px) in img.pixels.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        let base = (*px as u32) / 17;
        let frac = (*px as u32) % 17;
        let t = BAYER8[y % 8][x % 8] as u32;
        let up = frac * 128 > 17 * (2 * t + 1);
        *px = ((base + u32::from(up)) * 17).min(255) as u8;
    }
}

/// Draw the two panel steps next to black as black and the two next to white as white. Text and chrome
/// then change only pure pixels, which do not flash. From Primitive Browser.
pub fn solidify(gray: &mut [u8]) {
    for px in gray.iter_mut() {
        if *px <= 34 {
            *px = 0;
        } else if *px >= 221 {
            *px = 255;
        }
    }
}

/// Icon source to tile: decode, fit into `w` × `h`, dither.
pub fn icon_tile(png_bytes: &[u8], w: u32, h: u32) -> Result<GrayImage, String> {
    let img = decode_png(png_bytes)?;
    let mut tile = fit(&img, w, h);
    dither16(&mut tile);
    Ok(tile)
}

/// FNV-1a, for cache file names keyed by the icon's source bytes and the tile size.
pub fn cache_key(bytes: &[u8], w: u32, h: u32) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes.iter().chain(w.to_le_bytes().iter()).chain(h.to_le_bytes().iter()) {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

const CACHE_MAGIC: &[u8; 4] = b"KLG1";

/// Cache file: magic, width and height (u32 little endian), then the pixels.
pub fn to_cache(img: &GrayImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + img.pixels.len());
    out.extend_from_slice(CACHE_MAGIC);
    out.extend_from_slice(&img.width.to_le_bytes());
    out.extend_from_slice(&img.height.to_le_bytes());
    out.extend_from_slice(&img.pixels);
    out
}

pub fn from_cache(bytes: &[u8]) -> Option<GrayImage> {
    if bytes.len() < 12 || &bytes[..4] != CACHE_MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let pixels = bytes[12..].to_vec();
    (pixels.len() as u64 == width as u64 * height as u64).then_some(GrayImage { width, height, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn decodes_rgba_over_white_and_gray() {
        let rgba = png_bytes(2, 1, png::ColorType::Rgba, &[0, 0, 0, 255, 0, 0, 0, 0]);
        assert_eq!(decode_png(&rgba).unwrap().pixels, vec![0, 255]);
        let gray = png_bytes(1, 1, png::ColorType::Grayscale, &[100]);
        assert_eq!(decode_png(&gray).unwrap().pixels, vec![100]);
        assert!(decode_png(b"not a png").is_err());
    }

    #[test]
    fn fit_keeps_shape_and_averages() {
        // 4x2 black/white columns into 2x2: one row of two mid-grays, centred vertically on white.
        let img = GrayImage { width: 4, height: 2, pixels: vec![0, 255, 0, 255, 0, 255, 0, 255] };
        let out = fit(&img, 2, 2);
        assert_eq!(out.pixels, vec![128, 128, 255, 255]);
        let big = GrayImage { width: 600, height: 920, pixels: vec![0; 600 * 920] };
        let tile = fit(&big, 240, 360);
        assert_eq!((tile.width, tile.height), (240, 360));
        assert_eq!(tile.pixels[(180 * 240 + 120) as usize], 0);
    }

    #[test]
    fn dither_uses_panel_steps_and_keeps_the_average() {
        let mut img = GrayImage { width: 8, height: 8, pixels: vec![128; 64] };
        dither16(&mut img);
        assert!(img.pixels.iter().all(|p| p % 17 == 0));
        let avg = img.pixels.iter().map(|&p| p as u32).sum::<u32>() / 64;
        assert!((avg as i32 - 128).abs() <= 3, "average {avg}");
    }

    #[test]
    fn solidify_pushes_extremes() {
        let mut g = [0, 20, 34, 51, 204, 221, 250];
        solidify(&mut g);
        assert_eq!(g, [0, 0, 0, 51, 204, 255, 255]);
    }

    #[test]
    fn cache_round_trips_and_keys_differ_by_size() {
        let img = GrayImage { width: 2, height: 1, pixels: vec![1, 2] };
        assert_eq!(from_cache(&to_cache(&img)), Some(img));
        assert_eq!(from_cache(b"KLG1\x02\0\0\0\x02\0\0\0\x01"), None);
        assert_ne!(cache_key(b"a", 1, 2), cache_key(b"a", 2, 1));
    }
}
