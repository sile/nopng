//! Property-based tests for the APNG encoder written with `noprop`.
//!
//! The properties here pair the crate's own `encode_animation` with
//! `decode_animation` and check the result against an expectation built
//! directly from the sampled frames. The encoder writes every frame as a
//! full-canvas `SOURCE`/`NONE` frame, so for that encoder the round trip is an
//! identity: what goes in (after widening to `Rgba16Be`) must come back out.
//!
//! Reproduce a failure by copying the seed printed in the failure report into
//! the `NOPNG_PROP_TEST_SEED` environment variable and rerunning the same test
//! with the same case budget.

use std::cell::Cell;

/// Case budget for the round-trip properties.
const CASES: usize = 150;
/// Smaller budget for the byte-fuzzing property, whose cases are heavier.
const FUZZ_CASES: usize = 200;
/// Largest canvas edge. The canvas is square, so the pixel count stays small
/// while still covering multi-row, multi-column scanlines.
const MAX_CANVAS: u32 = 4;
/// Largest frame count. `1` and `2..=4` are the two structurally different
/// layouts the encoder emits (single `fcTL`/`IDAT`, versus `fcTL`/`IDAT`
/// followed by `fcTL`/`fdAT` pairs), so both must be reachable.
const MAX_FRAMES: u32 = 4;

// ---------------------------------------------------------------------
// Samplers (ordinary functions, not a combinator DSL).
// ---------------------------------------------------------------------

/// A square canvas edge in `1..=MAX_CANVAS`. APNG forbids a zero dimension, so
/// the range starts at one and no rejection is needed.
fn sample_canvas(ctx: &mut noprop::TestCaseContext) -> u32 {
    noprop::sample_u32_in(ctx, 1..=MAX_CANVAS)
}

/// A frame count in `1..=MAX_FRAMES`: 30% of the time exactly one frame, which
/// is the layout without any `fdAT` chunk.
fn sample_frame_count(ctx: &mut noprop::TestCaseContext) -> usize {
    noprop::sample_with_boundaries(ctx, &[1], noprop::Ratio::new(3, 10), |ctx| {
        noprop::sample_u32_in(ctx, 2..=MAX_FRAMES) as usize
    })
}

/// A play count covering both `acTL` encodings: `0` for loop-forever and a
/// non-zero count for `Finite`. Boundaries include the values either side of
/// `u16::MAX` and the `u32` maximum.
fn sample_play_count(ctx: &mut noprop::TestCaseContext) -> nopng::PlayCount {
    noprop::sample_with_boundaries(
        ctx,
        &[
            nopng::PlayCount::Infinite,
            nopng::PlayCount::ONCE,
            nopng::PlayCount::Finite(
                core::num::NonZeroU32::new(u32::MAX).expect("u32::MAX is non-zero"),
            ),
        ],
        noprop::Ratio::one_nth(2),
        |ctx| match noprop::sample_u32_in(ctx, 1..=1_000) {
            1 => nopng::PlayCount::ONCE,
            n => {
                nopng::PlayCount::Finite(core::num::NonZeroU32::new(n).expect("range starts at 1"))
            }
        },
    )
}

/// A delay numerator. `0` is legal on the wire (the frame is instant), and
/// `u16::MAX` is the largest value the chunk can carry.
fn sample_delay_num(ctx: &mut noprop::TestCaseContext) -> u16 {
    noprop::sample_with_boundaries(ctx, &[0, 1, u16::MAX], noprop::Ratio::one_nth(4), |ctx| {
        noprop::sample_u16_in(ctx, 0..=1_000)
    })
}

/// A delay denominator. `1` is included so the normalisation from `0` to `100`
/// is not the only visible case.
fn sample_delay_den(ctx: &mut noprop::TestCaseContext) -> core::num::NonZeroU16 {
    let value =
        noprop::sample_with_boundaries(ctx, &[1, 2, u16::MAX], noprop::Ratio::one_nth(4), |ctx| {
            noprop::sample_u16_in(ctx, 1..=1_000)
        });
    core::num::NonZeroU16::new(value).expect("every branch samples 1..=u16::MAX")
}

/// The eight formats that round-trip through `decode_image` unchanged, plus
/// indexed formats carrying a palette and a `tRNS` table.
///
/// `Rgb8`/`Rgb16Be`/`Gray*` without `tRNS` come back with the same format, so
/// the comparison against the expectation is exact. Formats that a decoder
/// would promote (a colour type with `tRNS`) are deliberately absent: the
/// encoder never writes `tRNS` for them, so they would not exercise anything
/// the plain variants miss.
fn sample_format(ctx: &mut noprop::TestCaseContext) -> nopng::PixelFormat {
    let choice = noprop::sample_weighted_index(ctx, &[1, 1, 1, 1, 1, 1, 1, 1, 2, 2]);
    let palette = |entries: usize| -> Vec<u8> {
        (0..entries)
            .flat_map(|index| {
                let index = index as u8;
                [index.wrapping_mul(17), 255 - index, 7]
            })
            .collect()
    };
    match choice {
        0 => nopng::PixelFormat::Rgba8,
        1 => nopng::PixelFormat::Rgba16Be,
        2 => nopng::PixelFormat::Rgb8,
        3 => nopng::PixelFormat::Rgb16Be,
        4 => nopng::PixelFormat::Gray8,
        5 => nopng::PixelFormat::Gray16Be,
        6 => nopng::PixelFormat::GrayAlpha8,
        7 => nopng::PixelFormat::GrayAlpha16Be,
        8 => nopng::PixelFormat::Indexed4 {
            palette: palette(16),
            trns: Some(vec![128; 16]),
        },
        _ => nopng::PixelFormat::Indexed8 {
            palette: palette(256),
            trns: None,
        },
    }
}

/// A sampled pixel buffer for `format` over a `canvas`x`canvas` image.
///
/// Every value in the buffer is legal for the format: indexed samples are drawn
/// from the palette's index range and low-bit grayscale from its value range,
/// so the encoder never has to reject the input.
fn sample_pixels(
    ctx: &mut noprop::TestCaseContext,
    format: &nopng::PixelFormat,
    canvas: u32,
) -> Vec<u8> {
    let len = format.data_len(canvas, canvas);
    match format {
        // The encoder rejects an index outside the palette, so draw each one
        // from the palette's own range instead of all 256 byte values. With a
        // full 16-entry palette this is the whole 0..=15 range.
        nopng::PixelFormat::Indexed4 { palette, .. } => {
            let entries = (palette.len() / 3) as u8;
            (0..len)
                .map(|_| noprop::sample_u8_in(ctx, 0..entries))
                .collect()
        }
        // Every other format `sample_format` produces is byte-per-sample for
        // 8-bit and indexed formats, or a 16-bit format whose two bytes are
        // both unconstrained, so every byte value is legal.
        _ => noprop::sample_bytes_vec(ctx, len),
    }
}

/// The `Rgba16Be` expectation for `pixels` in `format`.
///
/// Independent of the codec: the conversion is spelled out per format so the
/// property does not simply call `reformat_pixels` and compare the codec with
/// itself.
fn expected_rgba16(format: &nopng::PixelFormat, pixels: &[u8]) -> Vec<u8> {
    // 8-bit samples are widened to 16 bits by replication, `v * 257`, which
    // maps 0 to 0 and 255 to 65535. Spelled out here so the expectation does
    // not call back into the crate's own widening helper.
    let widen = |sample: u8| u16::from(sample) * 257;
    match format {
        nopng::PixelFormat::Rgba8 => pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], p[3]])
            .flat_map(|byte| widen(byte).to_be_bytes())
            .collect(),
        nopng::PixelFormat::Rgba16Be => pixels.to_vec(),
        nopng::PixelFormat::Rgb8 => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .flat_map(|byte| widen(byte).to_be_bytes())
            .collect(),
        nopng::PixelFormat::Rgb16Be => pixels
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|p| {
                [
                    u16::from_be_bytes([p[0], p[1]]),
                    u16::from_be_bytes([p[2], p[3]]),
                    u16::from_be_bytes([p[4], p[5]]),
                    u16::MAX,
                ]
            })
            .flat_map(u16::to_be_bytes)
            .collect(),
        nopng::PixelFormat::Gray8 => pixels
            .iter()
            .flat_map(|&g| [widen(g), widen(g), widen(g), u16::MAX])
            .flat_map(u16::to_be_bytes)
            .collect(),
        nopng::PixelFormat::Gray16Be => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_be_bytes(*p))
            .flat_map(|g| [g, g, g, u16::MAX])
            .flat_map(u16::to_be_bytes)
            .collect(),
        nopng::PixelFormat::GrayAlpha8 => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .flat_map(|byte| widen(byte).to_be_bytes())
            .collect(),
        nopng::PixelFormat::GrayAlpha16Be => pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let g = u16::from_be_bytes([p[0], p[1]]);
                let a = u16::from_be_bytes([p[2], p[3]]);
                [g, g, g, a]
            })
            .flat_map(u16::to_be_bytes)
            .collect(),
        nopng::PixelFormat::Indexed4 { palette, trns } => {
            palette_samples(pixels, palette, trns.as_deref(), widen)
        }
        nopng::PixelFormat::Indexed8 { palette, trns } => {
            palette_samples(pixels, palette, trns.as_deref(), widen)
        }
        other => panic!("sample_format never produces {other:?}"),
    }
}

/// Expands `indices` through `palette` and `trns`.
fn palette_samples(
    indices: &[u8],
    palette: &[u8],
    trns: Option<&[u8]>,
    widen: impl Fn(u8) -> u16,
) -> Vec<u8> {
    indices
        .iter()
        .flat_map(|&raw| {
            let index = usize::from(raw);
            let offset = index * 3;
            let alpha = trns
                .and_then(|table| table.get(index).copied())
                .unwrap_or(255);
            [
                palette[offset],
                palette[offset + 1],
                palette[offset + 2],
                alpha,
            ]
        })
        .flat_map(|byte| widen(byte).to_be_bytes())
        .collect()
}

/// Builds an [`nopng::Animation`] from a canvas size, format, play count, and delays.
fn animation(
    canvas: u32,
    format: nopng::PixelFormat,
    play_count: nopng::PlayCount,
    frames: Vec<(Vec<u8>, nopng::Delay)>,
) -> nopng::Animation {
    nopng::Animation {
        width: core::num::NonZeroU32::new(canvas).expect("canvas is 1..=MAX_CANVAS"),
        height: core::num::NonZeroU32::new(canvas).expect("canvas is 1..=MAX_CANVAS"),
        pixel_format: format,
        play_count,
        frames: frames
            .into_iter()
            .map(|(pixels, delay)| nopng::Frame { pixels, delay })
            .collect(),
    }
}

// ---------------------------------------------------------------------
// Properties.
// ---------------------------------------------------------------------

#[test]
fn roundtrip_random_animation() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    // Each of these is a structurally different decision the encoder or the
    // decoder makes, so the property is not allowed to pass by covering only
    // one side of any of them.
    let one_frame_cases = Cell::new(0usize);
    let multi_frame_cases = Cell::new(0usize);
    let infinite_play_cases = Cell::new(0usize);
    let finite_play_cases = Cell::new(0usize);
    let den_one_cases = Cell::new(0usize);
    let indexed_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let canvas = sample_canvas(ctx);
        let format = sample_format(ctx);
        let play_count = sample_play_count(ctx);
        let frame_count = sample_frame_count(ctx);

        let mut frames = Vec::with_capacity(frame_count);
        let mut expected = Vec::with_capacity(frame_count);
        let mut delays = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            let pixels = sample_pixels(ctx, &format, canvas);
            let delay = nopng::Delay {
                num: sample_delay_num(ctx),
                den: sample_delay_den(ctx),
            };
            expected.push(expected_rgba16(&format, &pixels));
            delays.push(delay);
            frames.push((pixels, delay));
        }

        let original = animation(canvas, format.clone(), play_count, frames);
        let encoded = nopng::encode_animation(&original).expect("sampled input is valid");
        let decoded = nopng::decode_animation(&encoded).expect("the encoder writes a valid APNG");

        assert_eq!(decoded.width.get(), canvas, "canvas width changed");
        assert_eq!(decoded.height.get(), canvas, "canvas height changed");
        assert_eq!(decoded.play_count, play_count, "play count changed");
        assert_eq!(decoded.frames.len(), frame_count, "frame count changed");
        for (index, (frame, expected_pixels)) in decoded.frames.iter().zip(&expected).enumerate() {
            assert_eq!(
                frame.pixels, *expected_pixels,
                "frame {index} pixels differ from the format-{format:?} expectation"
            );
            assert_eq!(frame.delay, delays[index], "frame {index} delay changed");
        }

        // Every case that reaches here has been compared on all of the above.
        if frame_count == 1 {
            one_frame_cases.set(one_frame_cases.get() + 1);
        } else {
            multi_frame_cases.set(multi_frame_cases.get() + 1);
        }
        match play_count {
            nopng::PlayCount::Infinite => infinite_play_cases.set(infinite_play_cases.get() + 1),
            nopng::PlayCount::Finite(_) => finite_play_cases.set(finite_play_cases.get() + 1),
        }
        if delays.iter().any(|delay| delay.den.get() == 1) {
            den_one_cases.set(den_one_cases.get() + 1);
        }
        if matches!(
            format,
            nopng::PixelFormat::Indexed4 { .. } | nopng::PixelFormat::Indexed8 { .. }
        ) {
            indexed_cases.set(indexed_cases.get() + 1);
        }
        Ok(())
    })?;

    assert!(
        one_frame_cases.get() > 0,
        "no case exercised the single-frame fcTL/IDAT layout\n{runner}"
    );
    assert!(
        multi_frame_cases.get() > 0,
        "no case exercised the fcTL/fdAT layout\n{runner}"
    );
    assert!(
        infinite_play_cases.get() > 0,
        "no case exercised an infinite play count\n{runner}"
    );
    assert!(
        finite_play_cases.get() > 0,
        "no case exercised a finite play count\n{runner}"
    );
    assert!(
        den_one_cases.get() > 0,
        "no case exercised a delay denominator of 1\n{runner}"
    );
    assert!(
        indexed_cases.get() > 0,
        "no case exercised an indexed format\n{runner}"
    );
    Ok(())
}

#[test]
fn roundtrip_preserves_every_play_count() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        // `acTL.num_plays` is a u32 where 0 means loop forever, so the
        // interesting values are the two ends of the finite range and the
        // transition from the 16-bit to the 32-bit magnitude.
        let value = noprop::sample_with_boundaries(
            ctx,
            &[1, 2, 65_535, 65_536, u32::MAX],
            noprop::Ratio::one_nth(3),
            |ctx| noprop::sample_u32_in(ctx, 1..=u32::MAX),
        );
        let play_count = if noprop::sample_bool(ctx) {
            nopng::PlayCount::Infinite
        } else {
            nopng::PlayCount::Finite(core::num::NonZeroU32::new(value).expect("range starts at 1"))
        };

        let pixels = noprop::sample_bytes_vec(ctx, 4);
        let original = animation(
            1,
            nopng::PixelFormat::Rgba8,
            play_count,
            vec![(
                pixels,
                nopng::Delay {
                    num: 1,
                    den: core::num::NonZeroU16::new(10).expect("10 is non-zero"),
                },
            )],
        );
        let encoded = nopng::encode_animation(&original).expect("sampled input is valid");
        let decoded = nopng::decode_animation(&encoded).expect("the encoder writes a valid APNG");
        assert_eq!(decoded.play_count, play_count, "play count changed");
        Ok(())
    })?;

    Ok(())
}

#[test]
fn encode_rejects_bad_input_as_invalid_data() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let empty_cases = Cell::new(0usize);
    let short_buffer_cases = Cell::new(0usize);
    let long_buffer_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let canvas = sample_canvas(ctx);
        let format = sample_format(ctx);
        let play_count = sample_play_count(ctx);
        let delay = nopng::Delay {
            num: sample_delay_num(ctx),
            den: sample_delay_den(ctx),
        };

        // Three invalid classes, chosen with enough weight each to be seen.
        match noprop::sample_weighted_index(ctx, &[1, 1, 1]) {
            0 => {
                let original = animation(canvas, format, play_count, Vec::new());
                let error = nopng::encode_animation(&original)
                    .expect_err("an animation needs at least one frame");
                assert!(
                    matches!(error, nopng::Error::InvalidData(_)),
                    "empty frame list gave {error:?}"
                );
                empty_cases.set(empty_cases.get() + 1);
            }
            1 => {
                let mut pixels = sample_pixels(ctx, &format, canvas);
                // A one-byte buffer (a 1x1 image in a single-byte format) has
                // nothing to remove, so shorten it by adding instead.
                if pixels.len() > 1 {
                    // Remove at least one byte but never all of them: an empty
                    // buffer is a different error path (a frame must exist),
                    // while this arm is about a wrong-length buffer for an
                    // existing frame.
                    let removable = (pixels.len() - 1).min(8);
                    let removed = noprop::sample_usize_in(ctx, 1..=removable);
                    pixels.truncate(pixels.len() - removed);
                    let original = animation(canvas, format, play_count, vec![(pixels, delay)]);
                    let error = nopng::encode_animation(&original)
                        .expect_err("a short pixel buffer must be rejected");
                    assert!(
                        matches!(error, nopng::Error::InvalidData(_)),
                        "short pixel buffer gave {error:?}"
                    );
                } else {
                    pixels.push(noprop::sample_u8(ctx));
                    let original = animation(canvas, format, play_count, vec![(pixels, delay)]);
                    let error = nopng::encode_animation(&original)
                        .expect_err("a long pixel buffer must be rejected");
                    assert!(
                        matches!(error, nopng::Error::InvalidData(_)),
                        "long pixel buffer gave {error:?}"
                    );
                }
                short_buffer_cases.set(short_buffer_cases.get() + 1);
            }
            _ => {
                let mut pixels = sample_pixels(ctx, &format, canvas);
                let extra = noprop::sample_usize_in(ctx, 1..=8);
                pixels.extend(core::iter::repeat_n(0, extra));
                let original = animation(canvas, format, play_count, vec![(pixels, delay)]);
                let error = nopng::encode_animation(&original)
                    .expect_err("a long pixel buffer must be rejected");
                assert!(
                    matches!(error, nopng::Error::InvalidData(_)),
                    "long pixel buffer gave {error:?}"
                );
                long_buffer_cases.set(long_buffer_cases.get() + 1);
            }
        }
        Ok(())
    })?;

    assert!(
        empty_cases.get() > 0,
        "no case exercised the empty animation\n{runner}"
    );
    assert!(
        short_buffer_cases.get() > 0,
        "no case exercised a short pixel buffer\n{runner}"
    );
    assert!(
        long_buffer_cases.get() > 0,
        "no case exercised a long pixel buffer\n{runner}"
    );
    assert_eq!(
        runner.stats().rejected_cases,
        0,
        "the generator rejects cases it should not\n{runner}"
    );
    Ok(())
}

#[test]
fn decoder_never_panics_on_arbitrary_bytes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(FUZZ_CASES, |ctx| {
        let len = noprop::sample_usize_in(ctx, 0..=2048);
        let data = noprop::sample_bytes_vec(ctx, len);
        // The only requirement is that neither entry point panics; an
        // `Unsupported` or `InvalidData` error is a perfectly good answer.
        let _ = nopng::decode_animation(&data);
        let _ = nopng::inspect(&data);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn roundtrip_is_stable_after_one_transcode() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let canvas = sample_canvas(ctx);
        let format = sample_format(ctx);
        let frame_count = sample_frame_count(ctx);
        let frames: Vec<(Vec<u8>, nopng::Delay)> = (0..frame_count)
            .map(|_| {
                (
                    sample_pixels(ctx, &format, canvas),
                    nopng::Delay {
                        num: sample_delay_num(ctx),
                        den: sample_delay_den(ctx),
                    },
                )
            })
            .collect();

        let first = animation(canvas, format, sample_play_count(ctx), frames);
        let bytes = nopng::encode_animation(&first).expect("sampled input is valid");
        let decoded = nopng::decode_animation(&bytes).expect("the encoder writes a valid APNG");

        // Re-encoding what was decoded must reproduce the same pixels, delays,
        // play count, and frame count. This is the metamorphic relation a
        // transcoder depends on: one pass through the crate is lossless, so a
        // second pass must not drift.
        let bytes_again =
            nopng::encode_animation(&decoded).expect("a decoded animation is encodable");
        let decoded_again =
            nopng::decode_animation(&bytes_again).expect("the second encode is valid");
        assert_eq!(decoded_again.width, decoded.width);
        assert_eq!(decoded_again.height, decoded.height);
        assert_eq!(decoded_again.play_count, decoded.play_count);
        assert_eq!(decoded_again.frames, decoded.frames);
        Ok(())
    })?;

    Ok(())
}
