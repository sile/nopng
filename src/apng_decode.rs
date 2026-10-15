//! Compositing of parsed APNG frames into full-canvas images.
//!
//! The chunk walker in [`crate::apng`] yields still-compressed frames plus the
//! rectangle, delay, dispose, and blend metadata that describes them. This
//! module turns that into the flat, composited output the public API exposes:
//! every frame is expanded to the full canvas and already combined with the
//! frames before it.
//!
//! Compositing always happens in `Rgba16Be`. That is the crate's lossless
//! common denominator, so mixing source formats (an 8-bit grayscale poster plus
//! a 16-bit RGBA frame, say) costs no precision. The finished canvas is
//! converted to the caller's requested format on the way out.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::apng::{BlendOp, DisposeOp, RawAnimation, RawFrame};
use crate::pixel_reformat::reformat;
use crate::png_decode::{PngHeader, decode_frame_pixels, decompress_zlib, expected_filtered_len};
use crate::png_types::{Error, PixelFormat, Result};

/// Bytes per pixel of the compositing format (`Rgba16Be`).
const CANVAS_BPP: usize = 8;

/// A frame's rectangle within the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Rect {
    fn of(frame: &RawFrame) -> Self {
        Self {
            x: frame.control.x_offset,
            y: frame.control.y_offset,
            width: frame.control.width,
            height: frame.control.height,
        }
    }

    fn is_full_canvas(&self, header: &PngHeader) -> bool {
        self.x == 0 && self.y == 0 && self.width == header.width && self.height == header.height
    }
}

/// One composited frame: full-canvas `Rgba16Be` pixels plus its delay.
pub(crate) struct CompositedFrame {
    pub(crate) pixels: Vec<u8>,
    pub(crate) delay_num: u16,
    pub(crate) delay_den: u16,
}

/// The result of compositing: the canvas size, the play count, and every frame
/// expanded to the full canvas in `Rgba16Be`.
pub(crate) struct CompositedAnimation {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) num_plays: u32,
    pub(crate) frames: Vec<CompositedFrame>,
}

/// Expands every frame of `raw` to the full canvas, in `Rgba16Be`.
pub(crate) fn composite(raw: RawAnimation) -> Result<CompositedAnimation> {
    let header = &raw.header;
    let canvas_len = canvas_len(header)?;
    // The canvas starts as fully transparent black and is never resized.
    let mut canvas = vec![0u8; canvas_len];

    let mut frames = Vec::with_capacity(raw.frames.len());
    for (index, frame) in raw.frames.iter().enumerate() {
        let rect = Rect::of(frame);
        check_rect_fits(rect, header)?;
        // The first *animation* frame must cover the whole canvas, whether it
        // is carried by IDAT or by fdAT (spec, "fcTL" section).
        if index == 0 && !rect.is_full_canvas(header) {
            return Err(Error::InvalidData(
                "the first animation frame must cover the whole canvas".into(),
            ));
        }

        let stream = frame_stream(frame, index, &raw)?;
        let frame_pixels_rgba16 = decode_frame_rgba16(header, &raw, frame, rect, stream)?;

        // `dispose_op == PREVIOUS` restores the rectangle to its contents
        // before this frame was drawn, so snapshot it first.
        let saved = if frame.dispose_op == DisposeOp::Previous {
            Some(snapshot_rect(&canvas, header, rect))
        } else {
            None
        };

        blend_into_canvas(
            &mut canvas,
            header,
            rect,
            &frame_pixels_rgba16,
            frame.blend_op,
        );

        frames.push(CompositedFrame {
            // Each frame gets its own copy of the canvas as it stands right
            // now, before this frame's dispose operation runs. The copy is
            // required because the next iteration mutates the canvas in place.
            pixels: canvas.clone(),
            delay_num: frame.control.delay_num,
            delay_den: frame.control.delay_den,
        });

        match frame.dispose_op {
            DisposeOp::None => {}
            DisposeOp::Background => clear_rect(&mut canvas, header, rect),
            DisposeOp::Previous => {
                let saved = saved.expect("bug: PREVIOUS must have snapshotted its rectangle");
                restore_rect(&mut canvas, header, rect, &saved);
            }
        }
    }

    Ok(CompositedAnimation {
        width: header.width,
        height: header.height,
        num_plays: raw.num_plays,
        frames,
    })
}

/// Returns the compressed stream that carries `frame`.
///
/// Frame 0 is carried by `IDAT` when the animation declares that layout; every
/// other frame carries its own `fdAT`-sourced stream.
fn frame_stream<'a>(frame: &'a RawFrame, index: usize, raw: &'a RawAnimation) -> Result<&'a [u8]> {
    if index == 0 && raw.idat_is_frame_zero {
        return Ok(&raw.idat_data);
    }
    if frame.zlib_stream.is_empty() {
        return Err(Error::InvalidData(
            format!("frame {index} has no compressed data").into(),
        ));
    }
    Ok(&frame.zlib_stream)
}

/// Inflates and decodes one frame into full-rectangle `Rgba16Be` pixels.
///
/// The frame is unfiltered at its own rectangle size and converted from the
/// image's native color model, then widened to `Rgba16Be` for blending.
fn decode_frame_rgba16(
    header: &PngHeader,
    raw: &RawAnimation,
    _frame: &RawFrame,
    rect: Rect,
    stream: &[u8],
) -> Result<Vec<u8>> {
    // The frame's scanlines are laid out for `rect.width` x `rect.height`, not
    // for the canvas, so the header driving the decode must be narrowed.
    let frame_header = PngHeader {
        width: rect.width,
        height: rect.height,
        ..*header
    };
    let expected = expected_filtered_len(&frame_header)?;
    let filtered = decompress_zlib(stream, expected)?;
    if filtered.len() != expected {
        return Err(Error::InvalidData(
            format!(
                "unexpected frame data size: expected {expected}, got {}",
                filtered.len()
            )
            .into(),
        ));
    }
    let (native_format, native_data) =
        decode_frame_pixels(&frame_header, &filtered, &raw.ancillary)?;
    let rgba16 = reformat(&native_format, &native_data, &PixelFormat::Rgba16Be)?;
    let expected_rgba = rect_len(rect)?;
    if rgba16.len() != expected_rgba {
        return Err(Error::InvalidData(
            format!(
                "unexpected composited frame size: expected {expected_rgba}, got {}",
                rgba16.len()
            )
            .into(),
        ));
    }
    Ok(rgba16)
}

/// Number of bytes in a full `Rgba16Be` canvas.
fn canvas_len(header: &PngHeader) -> Result<usize> {
    (header.width as usize)
        .checked_mul(header.height as usize)
        .and_then(|pixels| pixels.checked_mul(CANVAS_BPP))
        .ok_or_else(|| Error::InvalidData("canvas size overflow".into()))
}

/// Number of bytes in a rectangle's `Rgba16Be` pixel data.
fn rect_len(rect: Rect) -> Result<usize> {
    (rect.width as usize)
        .checked_mul(rect.height as usize)
        .and_then(|pixels| pixels.checked_mul(CANVAS_BPP))
        .ok_or_else(|| Error::InvalidData("frame size overflow".into()))
}

/// Rejects rectangles that are empty or extend past the canvas.
fn check_rect_fits(rect: Rect, header: &PngHeader) -> Result<()> {
    if rect.width == 0 || rect.height == 0 {
        return Err(Error::InvalidData(
            "frame width and height must be non-zero".into(),
        ));
    }
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or_else(|| Error::InvalidData("frame rectangle overflow".into()))?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or_else(|| Error::InvalidData("frame rectangle overflow".into()))?;
    if right > header.width || bottom > header.height {
        return Err(Error::InvalidData(
            format!(
                "frame rectangle {}x{}+{}+{} exceeds the {}x{} canvas",
                rect.width, rect.height, rect.x, rect.y, header.width, header.height
            )
            .into(),
        ));
    }
    Ok(())
}

/// Copies `rect` out of the canvas as `Rgba16Be` bytes.
fn snapshot_rect(canvas: &[u8], header: &PngHeader, rect: Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity(rect_len(rect).expect("bug: checked by check_rect_fits"));
    for row in 0..rect.height as usize {
        let y = rect.y as usize + row;
        let start = (y * header.width as usize + rect.x as usize) * CANVAS_BPP;
        let end = start + rect.width as usize * CANVAS_BPP;
        out.extend_from_slice(&canvas[start..end]);
    }
    out
}

/// Writes `saved` (produced by [`snapshot_rect`]) back into `rect`.
fn restore_rect(canvas: &mut [u8], header: &PngHeader, rect: Rect, saved: &[u8]) {
    for row in 0..rect.height as usize {
        let y = rect.y as usize + row;
        let start = (y * header.width as usize + rect.x as usize) * CANVAS_BPP;
        let end = start + rect.width as usize * CANVAS_BPP;
        let src = row * rect.width as usize * CANVAS_BPP;
        canvas[start..end].copy_from_slice(&saved[src..src + rect.width as usize * CANVAS_BPP]);
    }
}

/// Resets `rect` to fully transparent black.
fn clear_rect(canvas: &mut [u8], header: &PngHeader, rect: Rect) {
    for row in 0..rect.height as usize {
        let y = rect.y as usize + row;
        let start = (y * header.width as usize + rect.x as usize) * CANVAS_BPP;
        let end = start + rect.width as usize * CANVAS_BPP;
        canvas[start..end].fill(0);
    }
}

/// Combines a frame's `Rgba16Be` pixels with the canvas using `blend_op`.
fn blend_into_canvas(
    canvas: &mut [u8],
    header: &PngHeader,
    rect: Rect,
    src: &[u8],
    blend_op: BlendOp,
) {
    for row in 0..rect.height as usize {
        let y = rect.y as usize + row;
        let canvas_row = y * header.width as usize;
        let src_row = row * rect.width as usize;
        for col in 0..rect.width as usize {
            let dst = (canvas_row + rect.x as usize + col) * CANVAS_BPP;
            let src_px = (src_row + col) * CANVAS_BPP;
            match blend_op {
                BlendOp::Source => {
                    canvas[dst..dst + CANVAS_BPP].copy_from_slice(&src[src_px..src_px + CANVAS_BPP])
                }
                BlendOp::Over => {
                    let blended = blend_over(
                        &canvas[dst..dst + CANVAS_BPP],
                        &src[src_px..src_px + CANVAS_BPP],
                    );
                    canvas[dst..dst + CANVAS_BPP].copy_from_slice(&blended);
                }
            }
        }
    }
}

/// Straight-alpha source-over compositing of two `Rgba16Be` pixels.
///
/// The APNG spec (and PNG generally) defines `OVER` with straight alpha, not
/// premultiplied, so the output alpha is `a_s + a_d * (1 - a_s)` and the color
/// channels are divided by it at the end.
///
/// The intermediate products reach `65535 * 65535 ≈ 4.3e9`, and their sum
/// doubles that, so the arithmetic is done in `u64`. `u32` would overflow on
/// the additions even though each individual product just fits.
fn blend_over(dst: &[u8], src: &[u8]) -> [u8; CANVAS_BPP] {
    const MAX: u64 = u16::MAX as u64;

    let [sr, sg, sb, sa] = read_rgba16(src).map(u64::from);
    let [dr, dg, db, da] = read_rgba16(dst).map(u64::from);

    // Fully opaque source: the result is exactly the source, no rounding.
    if sa == MAX {
        let mut out = [0u8; CANVAS_BPP];
        out.copy_from_slice(src);
        return out;
    }

    // out_a = a_s + a_d * (1 - a_s)
    let out_a = sa + div_round(da * (MAX - sa), MAX);
    let out_r = composite_channel(sr, sa, dr, da, out_a, MAX);
    let out_g = composite_channel(sg, sa, dg, da, out_a, MAX);
    let out_b = composite_channel(sb, sa, db, da, out_a, MAX);

    let mut out = [0u8; CANVAS_BPP];
    for (index, value) in [out_r, out_g, out_b, out_a].into_iter().enumerate() {
        out[index * 2..index * 2 + 2].copy_from_slice(&(value as u16).to_be_bytes());
    }
    out
}

/// One color channel of source-over blending with straight alpha.
///
/// `out_c = (c_s * a_s + c_d * a_d * (1 - a_s)) / out_a`, with the division
/// guarded because a fully transparent result has an undefined color that is
/// conventionally written as zero.
fn composite_channel(src_c: u64, src_a: u64, dst_c: u64, dst_a: u64, out_a: u64, max: u64) -> u64 {
    if out_a == 0 {
        return 0;
    }
    let weighted_src = src_c * src_a;
    let dst_weight = div_round(dst_a * (max - src_a), max);
    let weighted_dst = dst_c * dst_weight;
    let sum = weighted_src + weighted_dst;
    div_round(sum, out_a)
}

/// Rounds `value / divisor` to nearest. `divisor` must be non-zero.
fn div_round(value: u64, divisor: u64) -> u64 {
    debug_assert!(divisor != 0);
    (value + divisor / 2) / divisor
}

fn read_rgba16(pixel: &[u8]) -> [u16; 4] {
    let mut out = [0u16; 4];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u16::from_be_bytes([pixel[index * 2], pixel[index * 2 + 1]]);
    }
    out
}
