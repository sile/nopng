//! APNG decoding tests: compositing, dispose/blend operations, delays, and
//! play counts.
//!
//! # How the fixtures are built
//!
//! `nopng` gains its own APNG *encoder* in a later change, so these tests build
//! APNG streams by hand. To avoid reimplementing zlib (and to keep the
//! compressed payloads honest), the helper below obtains a real `IDAT` zlib
//! stream from the crate's static [`nopng::encode_image`] and then re-wraps it
//! as `IDAT`/`fdAT` chunks. That way the decoder is exercised against streams
//! produced by the same deflate implementation it has to read, while every
//! APNG-specific decision (chunk order, rects, dispose/blend, delays) is made
//! independently here rather than by the code under test.

use core::num::{NonZeroU16, NonZeroU32};

/// `acTL`/`fcTL` dispose operation values, mirroring the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dispose {
    None = 0,
    Background = 1,
    Previous = 2,
}

/// `fcTL` blend operation values, mirroring the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blend {
    Source = 0,
    Over = 1,
}

/// One frame to place in a hand-built APNG.
#[derive(Debug, Clone)]
struct FrameSpec {
    /// Left edge of the frame's rectangle within the canvas.
    x: u32,
    /// Top edge of the frame's rectangle within the canvas.
    y: u32,
    /// Width of the frame's rectangle.
    width: u32,
    /// Height of the frame's rectangle.
    height: u32,
    delay_num: u16,
    delay_den: u16,
    dispose: Dispose,
    blend: Blend,
    /// Row-major RGBA8 pixels for this frame's rectangle.
    pixels: Vec<u8>,
}

impl FrameSpec {
    /// A full-canvas, single-colour RGBA8 frame.
    fn solid_fill(canvas: u32, rgba: [u8; 4], dispose: Dispose, blend: Blend) -> Self {
        Self {
            x: 0,
            y: 0,
            width: canvas,
            height: canvas,
            delay_num: 1,
            delay_den: 10,
            dispose,
            blend,
            pixels: solid(canvas, rgba),
        }
    }

    /// A full-canvas RGBA8 frame with explicit pixels.
    fn full(canvas: u32, pixels: Vec<u8>, dispose: Dispose, blend: Blend) -> Self {
        Self {
            x: 0,
            y: 0,
            width: canvas,
            height: canvas,
            delay_num: 1,
            delay_den: 10,
            dispose,
            blend,
            pixels,
        }
    }
}

fn crc(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc(&crc_input).to_be_bytes());
}

/// Returns the raw `IDAT` payload of a single-frame PNG, i.e. the zlib stream.
fn idat_of(png: &[u8]) -> Vec<u8> {
    let mut offset = 8;
    while offset + 12 <= png.len() {
        let len = u32::from_be_bytes(
            png[offset..offset + 4]
                .try_into()
                .expect("4 bytes from a 4-byte slice"),
        ) as usize;
        let kind = &png[offset + 4..offset + 8];
        if kind == b"IDAT" {
            return png[offset + 8..offset + 8 + len].to_vec();
        }
        offset += 12 + len;
    }
    panic!("no IDAT in encoded png");
}

/// The IHDR payload of an encoded PNG (width, height, bit depth, color type).
fn ihdr_of(png: &[u8]) -> [u8; 13] {
    let len =
        u32::from_be_bytes(png[8..12].try_into().expect("4 bytes from a 4-byte slice")) as usize;
    assert_eq!(&png[12..16], b"IHDR");
    let mut out = [0u8; 13];
    out.copy_from_slice(&png[16..16 + len]);
    out
}

/// Encodes one rectangle of RGBA8 pixels to a PNG and returns its zlib stream.
fn zlib_for_rect(frame: &FrameSpec, interlaced: bool) -> Vec<u8> {
    let mut spec = nopng::ImageSpec::new(frame.width, frame.height, nopng::PixelFormat::Rgba8);
    spec.interlaced = interlaced;
    let png = nopng::encode_image(&spec, &frame.pixels).expect("infallible");
    idat_of(&png)
}

/// Builds a complete APNG byte stream.
///
/// `poster` decides the layout: when `true`, the first `fcTL` follows `IDAT`,
/// so `IDAT` is a poster image that is not part of the animation. When `false`,
/// the first `fcTL` precedes `IDAT`, so `IDAT` carries frame 0.
///
/// `num_plays` is written verbatim into `acTL` (`0` means loop forever).
/// `declared_frames` is written as `acTL.num_frames`; pass `None` to write the
/// true frame count.
fn build_apng(
    canvas: u32,
    frames: &[FrameSpec],
    num_plays: u32,
    poster: bool,
    declared_frames: Option<u32>,
) -> Vec<u8> {
    build_apng_interlaced(canvas, frames, num_plays, poster, declared_frames, false)
}

/// Like [`build_apng`], but writes an interlaced (Adam7) stream.
///
/// Interlacing is an `IHDR` property, so it applies to every frame rather than
/// to individual `fcTL` records.
fn build_apng_interlaced(
    canvas: u32,
    frames: &[FrameSpec],
    num_plays: u32,
    poster: bool,
    declared_frames: Option<u32>,
    interlaced: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR: reuse the static encoder's header for the first frame so the bit
    // depth and color type are guaranteed consistent with the pixel data.
    let first = &frames[0];
    let mut probe_spec =
        nopng::ImageSpec::new(first.width, first.height, nopng::PixelFormat::Rgba8);
    probe_spec.interlaced = interlaced;
    let probe = nopng::encode_image(&probe_spec, &first.pixels).expect("infallible");
    let mut ihdr = ihdr_of(&probe);
    ihdr[0..4].copy_from_slice(&canvas.to_be_bytes());
    ihdr[4..8].copy_from_slice(&canvas.to_be_bytes());
    push_chunk(&mut out, b"IHDR", &ihdr);

    let num_frames = declared_frames.unwrap_or(frames.len() as u32);
    let mut actl = Vec::new();
    actl.extend_from_slice(&num_frames.to_be_bytes());
    actl.extend_from_slice(&num_plays.to_be_bytes());
    push_chunk(&mut out, b"acTL", &actl);

    let mut sequence = 0u32;
    let push_fctl = |out: &mut Vec<u8>, frame: &FrameSpec, seq: &mut u32| {
        let mut data = Vec::with_capacity(26);
        data.extend_from_slice(&seq.to_be_bytes());
        *seq += 1;
        data.extend_from_slice(&frame.width.to_be_bytes());
        data.extend_from_slice(&frame.height.to_be_bytes());
        data.extend_from_slice(&frame.x.to_be_bytes());
        data.extend_from_slice(&frame.y.to_be_bytes());
        data.extend_from_slice(&frame.delay_num.to_be_bytes());
        data.extend_from_slice(&frame.delay_den.to_be_bytes());
        data.push(frame.dispose as u8);
        data.push(frame.blend as u8);
        push_chunk(out, b"fcTL", &data);
    };

    if !poster {
        push_fctl(&mut out, first, &mut sequence);
        // `IDAT` carries frame 0's stream.
        push_chunk(&mut out, b"IDAT", &zlib_for_rect(first, interlaced));
    } else {
        // A poster image with the same canvas shape; not part of the animation.
        push_chunk(&mut out, b"IDAT", &zlib_for_rect(first, interlaced));
    }

    let start = usize::from(!poster);
    for frame in &frames[start..] {
        push_fctl(&mut out, frame, &mut sequence);
        // Split into two fdAT chunks to exercise fragment reassembly.
        let stream = zlib_for_rect(frame, interlaced);
        let split = stream.len() / 2;
        for fragment in [&stream[..split], &stream[split..]] {
            let mut data = Vec::with_capacity(4 + fragment.len());
            data.extend_from_slice(&sequence.to_be_bytes());
            sequence += 1;
            data.extend_from_slice(fragment);
            push_chunk(&mut out, b"fdAT", &data);
        }
    }

    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// Decodes to RGBA8 and returns `(delay_pairs, play_count, frames)`.
fn decode_rgba8(bytes: &[u8]) -> (nopng::Animation, Vec<(u16, u16)>) {
    let animation =
        nopng::decode_animation(bytes, nopng::PixelFormat::Rgba8).expect("decode must succeed");
    let delays = animation
        .frames
        .iter()
        .map(|f| (f.delay.num, f.delay.den.get()))
        .collect();
    (animation, delays)
}

/// A 2x2 canvas of opaque red, as RGBA8, plus a helper to make other colours.
fn solid(canvas: u32, rgba: [u8; 4]) -> Vec<u8> {
    rgba.iter()
        .copied()
        .cycle()
        .take((canvas * canvas) as usize * 4)
        .collect()
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[test]
fn decodes_idat_as_frame_zero() {
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let second = FrameSpec::full(2, solid(2, GREEN), Dispose::None, Blend::Source);
    let bytes = build_apng(2, &[first, second], 0, false, None);

    let (animation, delays) = decode_rgba8(&bytes);
    assert_eq!(animation.width, 2);
    assert_eq!(animation.height, 2);
    assert_eq!(animation.play_count, nopng::PlayCount::Infinite);
    assert_eq!(animation.frames.len(), 2);
    assert_eq!(animation.frames[0].pixels, solid(2, RED));
    assert_eq!(animation.frames[1].pixels, solid(2, GREEN));
    assert_eq!(delays, vec![(1, 10), (1, 10)]);
}

#[test]
fn decodes_poster_layout() {
    // With a poster, IDAT is a full-canvas image that must NOT appear as a
    // frame; every frame comes from fdAT.
    let first = FrameSpec::full(2, solid(2, GREEN), Dispose::None, Blend::Source);
    let second = FrameSpec::full(2, solid(2, BLUE), Dispose::None, Blend::Source);
    let bytes = build_apng(2, &[first, second], 0, true, None);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(animation.frames.len(), 2);
    assert_eq!(animation.frames[0].pixels, solid(2, GREEN));
    assert_eq!(animation.frames[1].pixels, solid(2, BLUE));
}

#[test]
fn overlay_uses_sub_rectangle() {
    let canvas = 2;
    // Frame 0: green canvas. Frame 1: a 1x1 red pixel drawn at (1, 0) with
    // BLEND OVER (opaque, so it replaces the destination pixel).
    let first = FrameSpec::full(canvas, solid(canvas, GREEN), Dispose::None, Blend::Source);
    let second = FrameSpec {
        x: 1,
        y: 0,
        width: 1,
        height: 1,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::None,
        blend: Blend::Over,
        pixels: RED.to_vec(),
    };
    let bytes = build_apng(canvas, &[first, second], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    // Frame 1 is the whole canvas: red at (1,0), green elsewhere.
    assert_eq!(
        animation.frames[1].pixels,
        vec![
            0, 255, 0, 255, /* (0,0) green */
            255, 0, 0, 255, /* (1,0) red   */
            0, 255, 0, 255, /* (0,1) green */
            0, 255, 0, 255, /* (1,1) green */
        ]
    );
}

#[test]
fn blend_over_alpha_composites_with_the_destination() {
    // Half-transparent red drawn OVER opaque green. This is the case that
    // distinguishes OVER from SOURCE: with plain overwriting the result would
    // be (255, 0, 0, 128), but source-over must keep the destination's green
    // contribution and produce an opaque pixel.
    //
    // Expected values from the spec formula with 16-bit channels (each 8-bit
    // sample v maps to v * 257):
    //   a_s = 128*257 = 32896,  a_d = 65535,  max = 65535
    //   out_a = 32896 + round(65535 * (65535 - 32896) / 65535) = 65535 -> 255
    //   out_r = round((65535 * 32896 + 0 * 32639) / 65535)    = 32896 -> 128
    //   out_g = round((0 * 32896 + 65535 * 32639) / 65535)    = 32639 -> 127
    let first = FrameSpec::full(2, solid(2, GREEN), Dispose::None, Blend::Source);
    let half_red = [255u8, 0, 0, 128];
    let second = FrameSpec::solid_fill(2, half_red, Dispose::None, Blend::Over);
    let bytes = build_apng(2, &[first, second], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    let expected = solid(2, [128, 127, 0, 255]);
    assert_eq!(
        animation.frames[1].pixels, expected,
        "OVER must composite alpha, not overwrite"
    );
}

#[test]
fn blend_over_keeps_destination_where_source_is_transparent() {
    // A fully transparent source must leave the destination untouched, which is
    // the other end of the OVER range.
    let first = FrameSpec::full(2, solid(2, BLUE), Dispose::None, Blend::Source);
    let transparent = [0u8, 0, 0, 0];
    let second = FrameSpec::solid_fill(2, transparent, Dispose::None, Blend::Over);
    let bytes = build_apng(2, &[first, second], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(animation.frames[1].pixels, solid(2, BLUE));
}

#[test]
fn blend_source_overwrites_alpha() {
    // SOURCE replaces the pixel outright, including its alpha.
    let first = FrameSpec::full(2, solid(2, BLUE), Dispose::None, Blend::Source);
    let transparent = [0u8, 0, 0, 0];
    let second = FrameSpec::solid_fill(2, transparent, Dispose::None, Blend::Source);
    let bytes = build_apng(2, &[first, second], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(animation.frames[1].pixels, solid(2, transparent));
}

#[test]
fn sub_rectangle_at_nonzero_y_offset_lands_on_the_right_row() {
    // A 1x1 red pixel at (0, 1) on a 2x2 green canvas. Using y != 0 exercises
    // the row-stride term of the offset arithmetic; an x-only offset would
    // pass even if the y term were dropped.
    let first = FrameSpec::full(2, solid(2, GREEN), Dispose::None, Blend::Source);
    let second = FrameSpec {
        x: 0,
        y: 1,
        width: 1,
        height: 1,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::None,
        blend: Blend::Source,
        pixels: RED.to_vec(),
    };
    let bytes = build_apng(2, &[first, second], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(
        animation.frames[1].pixels,
        vec![
            0, 255, 0, 255, /* (0,0) green */
            0, 255, 0, 255, /* (1,0) green */
            255, 0, 0, 255, /* (0,1) red   */
            0, 255, 0, 255, /* (1,1) green */
        ]
    );
}

#[test]
fn dispose_background_clears_only_the_frame_rectangle() {
    // A 1x1 frame at (1,1) with BACKGROUND dispose clears only that pixel,
    // leaving the rest of the canvas intact for the next frame.
    let canvas = 2;
    let first = FrameSpec::full(canvas, solid(canvas, GREEN), Dispose::None, Blend::Source);
    let patch = FrameSpec {
        x: 1,
        y: 1,
        width: 1,
        height: 1,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::Background,
        blend: Blend::Source,
        pixels: RED.to_vec(),
    };
    // A fully transparent OVER frame observes the post-dispose canvas without
    // changing it.
    let observer = FrameSpec::solid_fill(canvas, [0, 0, 0, 0], Dispose::None, Blend::Over);
    let bytes = build_apng(canvas, &[first, patch, observer], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(
        animation.frames[2].pixels,
        vec![
            0, 255, 0, 255, /* (0,0) green, untouched */
            0, 255, 0, 255, /* (1,0) green, untouched */
            0, 255, 0, 255, /* (0,1) green, untouched */
            0, 0, 0, 0, /* (1,1) cleared to transparent black */
        ]
    );
}

#[test]
fn dispose_background_clears_the_rectangle() {
    let canvas = 2;
    // Frame 0 draws red over a 1x1 rect at (0,0) and disposes to BACKGROUND,
    // so frame 1 starts from a transparent canvas.
    let first = FrameSpec {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::Background,
        blend: Blend::Source,
        pixels: solid(canvas, RED),
    };
    let second = FrameSpec::full(canvas, solid(canvas, BLUE), Dispose::None, Blend::Source);
    let bytes = build_apng(canvas, &[first, second], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(animation.frames[0].pixels, solid(canvas, RED));
    assert_eq!(animation.frames[1].pixels, solid(canvas, BLUE));
}

#[test]
fn dispose_previous_restores_the_rectangle() {
    let canvas = 2;
    // Frame 0: red canvas.
    // Frame 1: blue 1x1 at (0,0), dispose PREVIOUS -> restores red at (0,0).
    // Frame 2: a 1x1 sub-rect that cannot cover the canvas is illegal for the
    // first frame but fine later; use a full-canvas transparent frame here so
    // the restored pixels are observable.
    let first = FrameSpec::full(canvas, solid(canvas, RED), Dispose::None, Blend::Source);
    let second = FrameSpec {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::Previous,
        blend: Blend::Source,
        pixels: BLUE.to_vec(),
    };
    // A fully transparent full-canvas frame: with source-over it changes
    // nothing, so frame 2 must equal the state after frame 1's dispose.
    let transparent = [0u8, 0, 0, 0];
    let third = FrameSpec::full(
        canvas,
        solid(canvas, transparent),
        Dispose::None,
        Blend::Over,
    );
    let bytes = build_apng(canvas, &[first, second, third], 0, false, None);

    let (animation, _) = decode_rgba8(&bytes);
    // Frame 1 shows blue at (0,0) with the rest red.
    assert_eq!(
        animation.frames[1].pixels,
        vec![
            0, 0, 255, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255
        ]
    );
    // Frame 2 must have the red pixel restored at (0,0).
    assert_eq!(animation.frames[2].pixels, solid(canvas, RED));
}

#[test]
fn zero_delay_denominator_normalizes_to_100() {
    let mut first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    first.delay_num = 5;
    first.delay_den = 0;
    let bytes = build_apng(2, &[first], 1, false, None);

    let (animation, delays) = decode_rgba8(&bytes);
    assert_eq!(delays, vec![(5, 100)]);
    assert_eq!(
        animation.frames[0].delay.den,
        NonZeroU16::new(100).expect("100 is non-zero")
    );
    assert_eq!(
        animation.play_count,
        nopng::PlayCount::Finite(NonZeroU32::new(1).expect("1 is non-zero"))
    );
}

#[test]
fn play_count_variants() {
    let frame = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);

    let infinite = build_apng(2, core::slice::from_ref(&frame), 0, false, None);
    assert_eq!(
        decode_rgba8(&infinite).0.play_count,
        nopng::PlayCount::Infinite
    );

    let once = build_apng(2, core::slice::from_ref(&frame), 1, false, None);
    assert_eq!(decode_rgba8(&once).0.play_count, nopng::PlayCount::ONCE);

    let many = build_apng(2, core::slice::from_ref(&frame), 7, false, None);
    assert_eq!(
        decode_rgba8(&many).0.play_count,
        nopng::PlayCount::Finite(NonZeroU32::new(7).expect("7 is non-zero"))
    );
}

#[test]
fn wrong_declared_frame_count_is_tolerated() {
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let second = FrameSpec::full(2, solid(2, GREEN), Dispose::None, Blend::Source);
    // acTL claims 99 frames; the chunk stream actually has 2.
    let bytes = build_apng(2, &[first, second], 0, false, Some(99));
    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(animation.frames.len(), 2);
}

#[test]
fn decodes_to_gray8() {
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let bytes = build_apng(2, &[first], 0, false, None);
    let animation =
        nopng::decode_animation(&bytes, nopng::PixelFormat::Gray8).expect("decode must succeed");
    assert_eq!(animation.pixel_format, nopng::PixelFormat::Gray8);
    assert_eq!(animation.frames[0].pixels.len(), 4);
    // Pure red -> the crate's luma weights give 76.
    assert_eq!(animation.frames[0].pixels, vec![76, 76, 76, 76]);
}

#[test]
fn indexed_output_is_unsupported() {
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let bytes = build_apng(2, &[first], 0, false, None);
    let error = nopng::decode_animation(
        &bytes,
        nopng::PixelFormat::Indexed8 {
            palette: vec![0, 0, 0],
            trns: None,
        },
    )
    .expect_err("indexed output must be rejected");
    assert!(
        matches!(error, nopng::Error::Unsupported(_)),
        "got {error:?}"
    );
}

#[test]
fn decodes_interlaced_sub_rectangle() {
    // Adam7 is computed over the *frame's* rectangle, not the canvas, so an
    // interlaced sub-rectangle is a distinct code path from an interlaced
    // full-canvas frame.
    let canvas = 4;
    let first = FrameSpec::solid_fill(canvas, GREEN, Dispose::None, Blend::Source);
    let patch = FrameSpec {
        x: 1,
        y: 1,
        width: 2,
        height: 2,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::None,
        blend: Blend::Source,
        pixels: solid(2, RED),
    };
    let bytes = build_apng_interlaced(canvas, &[first, patch], 0, false, None, true);

    let (animation, _) = decode_rgba8(&bytes);
    let second = &animation.frames[1].pixels;
    // The 2x2 red block occupies pixels (1,1)..(2,2); the rest stays green.
    for y in 0..canvas as usize {
        for x in 0..canvas as usize {
            let offset = (y * canvas as usize + x) * 4;
            let pixel = &second[offset..offset + 4];
            let expected = if (1..3).contains(&x) && (1..3).contains(&y) {
                RED
            } else {
                GREEN
            };
            assert_eq!(pixel, &expected, "pixel ({x},{y})");
        }
    }
}

#[test]
fn decodes_interlaced_frames() {
    // Adam7 is an IHDR-level property, so every frame is interlaced. The
    // decoder must un-scatter each frame before compositing.
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let second = FrameSpec::full(2, solid(2, GREEN), Dispose::None, Blend::Source);
    let bytes = build_apng_interlaced(2, &[first, second], 0, false, None, true);

    let (animation, _) = decode_rgba8(&bytes);
    assert_eq!(animation.frames.len(), 2);
    assert_eq!(animation.frames[0].pixels, solid(2, RED));
    assert_eq!(animation.frames[1].pixels, solid(2, GREEN));
}

#[test]
fn static_png_is_not_an_animation() {
    let png = nopng::encode_image(
        &nopng::ImageSpec::new(2, 2, nopng::PixelFormat::Rgba8),
        &solid(2, RED),
    )
    .expect("infallible");
    let error = nopng::decode_animation(&png, nopng::PixelFormat::Rgba8)
        .expect_err("a static PNG has no acTL");
    assert!(
        matches!(error, nopng::Error::InvalidData(_)),
        "got {error:?}"
    );
}

#[test]
fn frame_larger_than_canvas_is_rejected() {
    // A 2x2 frame declared on a 1x1 canvas: the rectangle does not fit.
    let oversized = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let bytes = build_apng(1, &[oversized], 0, false, None);
    let error = nopng::decode_animation(&bytes, nopng::PixelFormat::Rgba8)
        .expect_err("a frame wider than the canvas must be rejected");
    assert!(
        matches!(error, nopng::Error::InvalidData(_)),
        "got {error:?}"
    );
}

#[test]
fn sub_rectangle_that_overflows_the_edge_is_rejected() {
    // A 2x2 canvas, then a 1x1 frame at x=2: it starts on the right edge and
    // extends past it.
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let overflowing = FrameSpec {
        x: 2,
        y: 0,
        width: 1,
        height: 1,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::None,
        blend: Blend::Source,
        pixels: GREEN.to_vec(),
    };
    let bytes = build_apng(2, &[first, overflowing], 0, false, None);
    let error = nopng::decode_animation(&bytes, nopng::PixelFormat::Rgba8)
        .expect_err("a frame past the right edge must be rejected");
    assert!(
        matches!(error, nopng::Error::InvalidData(_)),
        "got {error:?}"
    );
}

#[test]
fn first_frame_must_cover_the_canvas() {
    let first = FrameSpec::full(2, solid(2, RED), Dispose::None, Blend::Source);
    let bytes = build_apng(2, &[first], 0, false, None);
    assert!(nopng::decode_animation(&bytes, nopng::PixelFormat::Rgba8).is_ok());

    // A 1x1 first frame on a 2x2 canvas is illegal.
    let sub = FrameSpec {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        delay_num: 1,
        delay_den: 10,
        dispose: Dispose::None,
        blend: Blend::Source,
        pixels: RED.to_vec(),
    };
    let bytes = build_apng(2, &[sub], 0, false, None);
    let error = nopng::decode_animation(&bytes, nopng::PixelFormat::Rgba8)
        .expect_err("a sub-rectangle first frame must be rejected");
    assert!(
        matches!(error, nopng::Error::InvalidData(_)),
        "got {error:?}"
    );
}
