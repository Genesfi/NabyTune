//! Cover art as GPU-ready images, and the ambient background made from it.
//!
//! The ambient image is computed in f32 from start to finish (downscale,
//! Gaussian blur, saturation, upscale) and only quantised to 8 bits at the very
//! end, with triangular (TPDF) dither. That is what removes the banding the
//! WebView version showed: the gradient never passes through an 8-bit step
//! until the last moment, and the dither spreads that last rounding below what
//! the eye can see.

use std::sync::{Arc, Mutex};

use gpui::RenderImage;
use image::{imageops::FilterType, Frame, RgbaImage};

/// The ambient image's size. The GPU stretches it to the window; it is smooth enough not to show.
const AMBIENT: u32 = 256;
/// The blur works on a thumbnail this size; at 512 px out, sigma 4.5 here is roughly 58 px.
const THUMB: u32 = 40;

/// Every image made here, so the ones nothing holds any more can be let go of: GPUI keeps
/// each image it has drawn in its GPU atlas until it's told to drop it, so without this every
/// cover ever shown stays in memory for as long as the app runs.
static MADE: Mutex<Vec<Arc<RenderImage>>> = Mutex::new(Vec::new());

/// RGBA → GPUI's BGRA frame.
pub fn render_image(mut rgba: RgbaImage) -> Arc<RenderImage> {
    for px in rgba.pixels_mut() {
        px.0.swap(0, 2);
    }
    let image = Arc::new(RenderImage::new(vec![Frame::new(rgba)]));
    MADE.lock().unwrap_or_else(|e| e.into_inner()).push(image.clone());
    image
}

/// An image made here, back as RGBA (it's kept BGRA for the GPU).
pub fn rgba_of(image: &RenderImage) -> Option<RgbaImage> {
    let size = image.size(0);
    let mut rgba = RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, image.as_bytes(0)?.to_vec())?;
    for px in rgba.pixels_mut() {
        px.0.swap(0, 2);
    }
    Some(rgba)
}

/// How many bytes an image's pixels take.
pub fn bytes_of(image: &RenderImage) -> usize {
    (0..image.frame_count()).map(|i| image.as_bytes(i).map_or(0, <[u8]>::len)).sum()
}

/// The images held by nothing but the list above: done with, for the atlas to drop. They're
/// out of the list once handed back.
pub fn unused_images() -> Vec<Arc<RenderImage>> {
    let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
    let mut unused = Vec::new();
    made.retain(|image| {
        let held = Arc::strong_count(image) > 1;
        if !held {
            unused.push(image.clone());
        }
        held
    });
    unused
}

/// The picture in an image, square: black letterbox bars trimmed (a video's 16:9 frame in
/// YouTube's 4:3 thumbnail), then its middle square. Squeezing a wide picture square would
/// flatten it.
pub fn square(source: &RgbaImage) -> RgbaImage {
    let (w, h) = source.dimensions();
    let dark_row = |y: u32| (0..w).step_by(4).all(|x| source.get_pixel(x, y).0[..3].iter().all(|c| *c < 24));
    let dark_col = |x: u32, top: u32, bottom: u32| (top..bottom).step_by(4).all(|y| source.get_pixel(x, y).0[..3].iter().all(|c| *c < 24));
    let mut top = 0;
    while top < h / 3 && dark_row(top) {
        top += 1;
    }
    let mut bottom = h;
    while bottom > h - h / 3 && dark_row(bottom - 1) {
        bottom -= 1;
    }
    let mut left = 0;
    while left < w / 3 && dark_col(left, top, bottom) {
        left += 1;
    }
    let mut right = w;
    while right > w - w / 3 && dark_col(right - 1, top, bottom) {
        right -= 1;
    }
    // An all-dark cover: keep it whole.
    if bottom <= top || right <= left {
        (top, bottom, left, right) = (0, h, 0, w);
    }
    let side = (right - left).min(bottom - top);
    let x = left + (right - left - side) / 2;
    let y = top + (bottom - top - side) / 2;
    image::imageops::crop_imm(source, x, y, side, side).to_image()
}

/// The cover at `size`×`size` for display.
pub fn cover(source: &RgbaImage, size: u32) -> Arc<RenderImage> {
    render_image(image::imageops::resize(&square(source), size, size, FilterType::Lanczos3))
}

/// Prepares a compact square cover image for real-time per-frame vinyl rotation (only ~256 KB).
pub fn vinyl_base(source: &RgbaImage, size: u32) -> Arc<RgbaImage> {
    Arc::new(image::imageops::resize(&square(source), size, size, FilterType::Triangle))
}

/// Renders ONE vinyl frame at an exact floating-point `angle_deg` using 16.16 fixed-point
/// bilinear interpolation (~0.15 ms per frame). Not pushed into `MADE`: caller drops the
/// previous frame directly via `window.drop_image` for constant O(1) RAM & GPU atlas usage.
pub fn vinyl_frame_at(base: &RgbaImage, angle_deg: f32) -> Arc<RenderImage> {
    let size = base.width().min(base.height());
    let sz = size as usize;
    let row_stride = sz * 4;
    let max_u = size.saturating_sub(1);
    let base_raw = base.as_raw();

    let w = size as f32;
    let cx = (w - 1.0) * 0.5;
    let cy = cx;
    let outer_r = w * 0.5 + 1.0;
    let outer_r2 = outer_r * outer_r;

    let rad = angle_deg.to_radians();
    let scale = 0.992f32;
    let cos_a = rad.cos() * scale;
    let sin_a = rad.sin() * scale;
    const FP: f32 = 65536.0;
    let cos_fp = (cos_a * FP) as i32;
    let sin_fp = (sin_a * FP) as i32;

    let mut out = RgbaImage::new(size, size);
    let out_raw: &mut [u8] = out.as_mut();

    for y in 0..size {
        let dy = y as f32 - cy;
        let dx_sq = outer_r2 - dy * dy;
        if dx_sq <= 0.0 {
            continue;
        }
        let dx_max = dx_sq.sqrt();
        let x_start = ((cx - dx_max).floor() as i32).max(0) as u32;
        let x_end = ((cx + dx_max).ceil() as i32).min(size as i32 - 1) as u32;

        let dx0 = x_start as f32 - cx;
        let mut sx_fp = ((cx + dx0 * cos_a + dy * sin_a) * FP) as i32;
        let mut sy_fp = ((cy - dx0 * sin_a + dy * cos_a) * FP) as i32;
        let row_out = (y as usize) * row_stride;

        for x in x_start..=x_end {
            let x0 = (sx_fp >> 16) as u32;
            let y0 = (sy_fp >> 16) as u32;
            if x0 < max_u && y0 < max_u {
                let fx = ((sx_fp >> 8) & 0xFF) as u32;
                let fy = ((sy_fp >> 8) & 0xFF) as u32;
                let ix = 256 - fx;
                let iy = 256 - fy;
                let w00 = ix * iy;
                let w10 = fx * iy;
                let w01 = ix * fy;
                let w11 = fx * fy;

                let i00 = (y0 as usize) * row_stride + (x0 as usize) * 4;
                let i10 = i00 + 4;
                let i01 = i00 + row_stride;
                let i11 = i01 + 4;

                let r = ((base_raw[i00] as u32 * w00
                    + base_raw[i10] as u32 * w10
                    + base_raw[i01] as u32 * w01
                    + base_raw[i11] as u32 * w11)
                    >> 16) as u8;
                let g = ((base_raw[i00 + 1] as u32 * w00
                    + base_raw[i10 + 1] as u32 * w10
                    + base_raw[i01 + 1] as u32 * w01
                    + base_raw[i11 + 1] as u32 * w11)
                    >> 16) as u8;
                let b = ((base_raw[i00 + 2] as u32 * w00
                    + base_raw[i10 + 2] as u32 * w10
                    + base_raw[i01 + 2] as u32 * w01
                    + base_raw[i11 + 2] as u32 * w11)
                    >> 16) as u8;

                let oi = row_out + (x as usize) * 4;
                // GPUI expects BGRA
                out_raw[oi] = b;
                out_raw[oi + 1] = g;
                out_raw[oi + 2] = r;
                out_raw[oi + 3] = 255;
            }
            sx_fp = sx_fp.wrapping_add(cos_fp);
            sy_fp = sy_fp.wrapping_sub(sin_fp);
        }
    }

    Arc::new(RenderImage::new(vec![Frame::new(out)]))
}

/// The cover, blurred far past recognition and spread to `AMBIENT` px, with alpha `alpha`.
pub fn ambient(source: &RgbaImage, alpha: f32) -> Arc<RenderImage> {
    let thumb = image::imageops::resize(&square(source), THUMB, THUMB, FilterType::Triangle);
    let n = THUMB as usize;
    let mut px: Vec<[f32; 3]> = thumb
        .pixels()
        .map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0])
        .collect();

    blur(&mut px, n, 4.5);

    // A little more colour, and dark enough that white text stays legible over it: a
    // pale cover (a white sleeve) would otherwise wash the whole window grey.
    const MAX_LUMA: f32 = 0.24;
    for c in &mut px {
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for v in c.iter_mut() {
            *v = (luma + (*v - luma) * 1.35).clamp(0.0, 1.0) * 0.62;
        }
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        if luma > MAX_LUMA {
            let k = MAX_LUMA / luma;
            c.iter_mut().for_each(|v| *v *= k);
        }
    }

    let out = AMBIENT as usize;
    let mut img = RgbaImage::new(AMBIENT, AMBIENT);
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    for y in 0..out {
        // Bilinear sample of the thumbnail at pixel centres, in f32.
        let fy = ((y as f32 + 0.5) / out as f32) * n as f32 - 0.5;
        let y0 = fy.floor().clamp(0.0, (n - 1) as f32) as usize;
        let y1 = (y0 + 1).min(n - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..out {
            let fx = ((x as f32 + 0.5) / out as f32) * n as f32 - 0.5;
            let x0 = fx.floor().clamp(0.0, (n - 1) as f32) as usize;
            let x1 = (x0 + 1).min(n - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let mut rgb = [0u8; 3];
            for ch in 0..3 {
                let top = px[y0 * n + x0][ch] * (1.0 - tx) + px[y0 * n + x1][ch] * tx;
                let bottom = px[y1 * n + x0][ch] * (1.0 - tx) + px[y1 * n + x1][ch] * tx;
                let v = (top * (1.0 - ty) + bottom * ty) * 255.0;
                // TPDF dither: the sum of two uniform values, ±1 LSB, triangular.
                let d = hash01(x as u32, y as u32, ch as u32 * 2) + hash01(x as u32, y as u32, ch as u32 * 2 + 1) - 1.0;
                rgb[ch] = (v + d).round().clamp(0.0, 255.0) as u8;
            }
            img.put_pixel(x as u32, y as u32, image::Rgba([rgb[0], rgb[1], rgb[2], a]));
        }
    }
    render_image(img)
}

/// The cover, lightly blurred so its shapes still read, darkened for text on top, at
/// 480 px: the full-screen player's "Blurred cover" background. Float all the way, dithered
/// at the end, like `ambient`.
pub fn soft(source: &RgbaImage) -> Arc<RenderImage> {
    const N: u32 = 72;
    let thumb = image::imageops::resize(&square(source), N, N, FilterType::Triangle);
    let n = N as usize;
    let mut px: Vec<[f32; 3]> = thumb.pixels().map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
    blur(&mut px, n, 1.6);
    for c in &mut px {
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for v in c.iter_mut() {
            *v = (luma + (*v - luma) * 1.2).clamp(0.0, 1.0) * 0.6;
        }
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        if luma > 0.3 {
            let k = 0.3 / luma;
            c.iter_mut().for_each(|v| *v *= k);
        }
    }
    render_image(upscale_dithered(&px, n, 480, 255))
}

/// Two colours from the cover for a gradient: the top-left's and the bottom-right's, a
/// little richer and dark enough for white text. As 0xRRGGBB.
pub fn palette(source: &RgbaImage) -> [u32; 2] {
    // The cover as a 4×4 grid of averaged patches.
    let thumb = image::imageops::resize(&square(source), 24, 24, FilterType::Triangle);
    let mut patches: Vec<[f32; 3]> = Vec::with_capacity(16);
    for gy in 0..4 {
        for gx in 0..4 {
            let mut sum = [0f32; 3];
            for y in gy * 6..gy * 6 + 6 {
                for x in gx * 6..gx * 6 + 6 {
                    let p = thumb.get_pixel(x, y);
                    (0..3).for_each(|ch| sum[ch] += p[ch] as f32 / 255.0);
                }
            }
            patches.push(sum.map(|v| v / 36.0));
        }
    }
    let chroma = |c: &[f32; 3]| c.iter().cloned().fold(0.0f32, f32::max) - c.iter().cloned().fold(1.0f32, f32::min);
    let hue = |c: &[f32; 3]| {
        let rgb = ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32;
        crate::appearance::hue_of(rgb)
    };
    // The cover's most vivid patch, then the most vivid one of another hue (a pale cover's
    // corners average to grey; its colours are where they are, not where the corners are).
    let first = patches.iter().cloned().max_by(|a, b| chroma(a).total_cmp(&chroma(b))).unwrap_or([0.2; 3]);
    let apart = |c: &[f32; 3]| {
        let d = (hue(c) - hue(&first)).abs();
        d.min(1.0 - d)
    };
    let second = patches
        .iter()
        .cloned()
        .filter(|c| apart(c) > 0.06)
        .max_by(|a, b| (chroma(a) * (0.5 + apart(a))).total_cmp(&(chroma(b) * (0.5 + apart(b)))))
        // One colour only: its darker patch.
        .unwrap_or_else(|| patches.iter().cloned().min_by(|a, b| a.iter().sum::<f32>().total_cmp(&b.iter().sum::<f32>())).unwrap_or(first));
    // Richer, and dark enough for white text.
    let finish = |mut c: [f32; 3]| {
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c = c.map(|v| (luma + (v - luma) * 1.8).clamp(0.0, 1.0));
        let luma = (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).max(0.001);
        let k = (0.24 / luma).min(0.9);
        c = c.map(|v| (v * k).clamp(0.0, 1.0));
        c.iter().fold(0u32, |acc, v| (acc << 8) | (v * 255.0).round() as u32)
    };
    [finish(first), finish(second)]
}

/// An accent from the cover: the hue of its most vivid patch, bright and rich enough for
/// buttons on a dark page. None for a cover without colour (black and white, greys).
pub fn accent_from(source: &RgbaImage) -> Option<u32> {
    let thumb = image::imageops::resize(&square(source), 16, 16, FilterType::Triangle);
    let mut best: Option<(f32, u32)> = None;
    for gy in 0..4 {
        for gx in 0..4 {
            let mut sum = [0f32; 3];
            for y in gy * 4..gy * 4 + 4 {
                for x in gx * 4..gx * 4 + 4 {
                    let p = thumb.get_pixel(x, y);
                    (0..3).for_each(|ch| sum[ch] += p[ch] as f32 / 255.0);
                }
            }
            let c = sum.map(|v| v / 16.0);
            let chroma = c.iter().cloned().fold(0.0f32, f32::max) - c.iter().cloned().fold(1.0f32, f32::min);
            let rgb = ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32;
            if best.is_none_or(|(b, _)| chroma > b) {
                best = Some((chroma, rgb));
            }
        }
    }
    let (chroma, rgb) = best?;
    (chroma > 0.12).then(|| crate::appearance::hsl(crate::appearance::hue_of(rgb), 0.62))
}

/// Bilinear upscale of an n×n f32 image to `out`×`out`, quantised with TPDF dither.
fn upscale_dithered(px: &[[f32; 3]], n: usize, out: u32, alpha: u8) -> RgbaImage {
    let size = out as usize;
    let mut img = RgbaImage::new(out, out);
    for y in 0..size {
        let fy = ((y as f32 + 0.5) / size as f32) * n as f32 - 0.5;
        let y0 = fy.floor().clamp(0.0, (n - 1) as f32) as usize;
        let y1 = (y0 + 1).min(n - 1);
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..size {
            let fx = ((x as f32 + 0.5) / size as f32) * n as f32 - 0.5;
            let x0 = fx.floor().clamp(0.0, (n - 1) as f32) as usize;
            let x1 = (x0 + 1).min(n - 1);
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            let mut rgb = [0u8; 3];
            for ch in 0..3 {
                let top = px[y0 * n + x0][ch] * (1.0 - tx) + px[y0 * n + x1][ch] * tx;
                let bottom = px[y1 * n + x0][ch] * (1.0 - tx) + px[y1 * n + x1][ch] * tx;
                let v = (top * (1.0 - ty) + bottom * ty) * 255.0;
                let d = hash01(x as u32, y as u32, ch as u32 * 2) + hash01(x as u32, y as u32, ch as u32 * 2 + 1) - 1.0;
                rgb[ch] = (v + d).round().clamp(0.0, 255.0) as u8;
            }
            img.put_pixel(x as u32, y as u32, image::Rgba([rgb[0], rgb[1], rgb[2], alpha]));
        }
    }
    img
}

/// Separable Gaussian blur over an n×n f32 image, edges clamped.
fn blur(px: &mut [[f32; 3]], n: usize, sigma: f32) {
    let radius = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f32> = (-radius..=radius).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|k| k / sum).collect();
    let clamp = |i: isize| i.clamp(0, n as isize - 1) as usize;

    let mut tmp = vec![[0f32; 3]; n * n];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0f32; 3];
            for (k, w) in kernel.iter().enumerate() {
                let sx = clamp(x as isize + k as isize - radius);
                for ch in 0..3 {
                    acc[ch] += px[y * n + sx][ch] * w;
                }
            }
            tmp[y * n + x] = acc;
        }
    }
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0f32; 3];
            for (k, w) in kernel.iter().enumerate() {
                let sy = clamp(y as isize + k as isize - radius);
                for ch in 0..3 {
                    acc[ch] += tmp[sy * n + x][ch] * w;
                }
            }
            px[y * n + x] = acc;
        }
    }
}

/// A stable pseudo-random value in [0, 1) per pixel and channel.
fn hash01(x: u32, y: u32, salt: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ salt.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn finds_colour_in_pale_covers() {
        // Mostly white and pale grey, with a patch of sky blue and one of grass green.
        let mut img = RgbaImage::from_pixel(240, 240, Rgba([235, 235, 232, 255]));
        for y in 0..60 {
            for x in 180..240 {
                img.put_pixel(x, y, Rgba([70, 150, 230, 255]));
            }
        }
        for y in 180..240 {
            for x in 0..60 {
                img.put_pixel(x, y, Rgba([90, 170, 70, 255]));
            }
        }
        let [a, b] = palette(&img);
        let chroma = |c: u32| {
            let ch = [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff];
            ch.iter().max().unwrap() - ch.iter().min().unwrap()
        };
        assert!(chroma(a) > 30 && chroma(b) > 30, "{a:06x} {b:06x}");
        assert!((crate::appearance::hue_of(a) - crate::appearance::hue_of(b)).abs() > 0.1);
    }

    #[test]
    fn squares_letterboxed_thumbnails() {
        // 480×360 with a 480×270 picture between black bars, as YouTube's hqdefault is.
        let mut img = RgbaImage::from_pixel(480, 360, Rgba([0, 0, 0, 255]));
        for y in 45..315 {
            for x in 0..480 {
                img.put_pixel(x, y, Rgba([200, 120, 40, 255]));
            }
        }
        let sq = square(&img);
        assert_eq!(sq.dimensions(), (270, 270));
        assert_eq!(sq.get_pixel(0, 0).0, [200, 120, 40, 255]);
        // A square cover stays as it is.
        let cover = RgbaImage::from_pixel(300, 300, Rgba([10, 200, 90, 255]));
        assert_eq!(square(&cover).dimensions(), (300, 300));
    }
}
