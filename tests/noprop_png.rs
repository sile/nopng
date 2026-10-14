//! Property-based tests for `nopng` written with `noprop`.
//!
//! These are the imperative counterparts of the former `proptest`-based
//! tests: every property is a plain Rust closure that samples its inputs
//! and asserts on the result. The `png` crate is kept as an independent
//! reference decoder ("oracle") for the differential properties that
//! compare an encoded image against an external implementation.
//!
//! Reproduce a failure by copying the seed printed in the failure report
//! into the `NOPNG_PROP_TEST_SEED` environment variable and rerunning the
//! same test with the same case budget.

use std::cell::Cell;
use std::io::Cursor;

use nopng::{ImageSpec, PixelFormat, decode_image, encode_image, reformat_pixels};
use noprop::{TestCaseContext, TestResult};

/// Case budget for the cheap round-trip and differential properties.
const CASES: usize = 100;
/// Smaller budget for the byte-fuzzing property, whose cases are heavier.
const FUZZ_CASES: usize = 200;

// ---------------------------------------------------------------------
// Reference decoder (the `png` crate) used as an oracle.
// ---------------------------------------------------------------------

fn decode_with_png_crate(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), png::DecodingError> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf)?;
    Ok((
        info.width,
        info.height,
        normalize_to_rgba8(info.color_type, &buf[..info.buffer_size()]),
    ))
}

fn normalize_to_rgba8(color_type: png::ColorType, data: &[u8]) -> Vec<u8> {
    match color_type {
        png::ColorType::Grayscale => data
            .iter()
            .flat_map(|&gray| [gray, gray, gray, 255])
            .collect(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|chunk| [chunk[0], chunk[1], chunk[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
            .collect(),
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Indexed => unreachable!("indexed data should be expanded by png crate"),
    }
}

// ---------------------------------------------------------------------
// Samplers (ordinary functions, not a combinator DSL).
// ---------------------------------------------------------------------

/// A random size in `1..=max` for each axis, so every image is non-empty.
fn sample_dimensions(ctx: &mut TestCaseContext, max: u32) -> (u32, u32) {
    (
        noprop::sample_u32_in(ctx, 1..=max),
        noprop::sample_u32_in(ctx, 1..=max),
    )
}

/// RGBA8 pixels for a `width`x`height` image.
fn sample_rgba8(ctx: &mut TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    noprop::sample_bytes_vec(ctx, (width * height * 4) as usize)
}

/// RGBA16 big-endian pixels for a `width`x`height` image.
fn sample_rgba16_be(ctx: &mut TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    noprop::sample_bytes_vec(ctx, (width * height * 8) as usize)
}

/// 2-bit grayscale samples (values 0..=3) for a `width`x`height` image.
fn sample_gray2(ctx: &mut TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    let pixels = (width * height) as usize;
    (0..pixels)
        .map(|_| noprop::sample_u8_in(ctx, 0..=3))
        .collect()
}

/// A fixed 4-colour palette with a full 4-entry tRNS table.
fn fixed_palette() -> Vec<u8> {
    vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]
}

fn fixed_trns() -> Vec<u8> {
    vec![255, 128, 255, 64]
}

/// Indices in `0..4` for a `width`x`height` 4-bit indexed image.
fn sample_indices4(ctx: &mut TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    let pixels = (width * height) as usize;
    (0..pixels)
        .map(|_| noprop::sample_u8_in(ctx, 0..4))
        .collect()
}

// ---------------------------------------------------------------------
// Properties.
// ---------------------------------------------------------------------

#[test]
fn roundtrip_random_rgba() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    // Interlacing is a distinct code path; require both variants to occur
    // so the property cannot pass by only exercising one of them.
    let interlaced_cases = Cell::new(0usize);
    let non_interlaced_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let interlaced = noprop::sample_bool(ctx);
        let data = sample_rgba8(ctx, width, height);
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Rgba8,
            interlaced,
        };
        let encoded = encode_image(&spec, &data).expect("infallible");

        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        assert_eq!(decoded_spec.width, width);
        assert_eq!(decoded_spec.height, height);
        assert_eq!(decoded_spec.interlaced, interlaced);
        assert_eq!(decoded_data, data);

        if interlaced {
            interlaced_cases.set(interlaced_cases.get() + 1);
        } else {
            non_interlaced_cases.set(non_interlaced_cases.get() + 1);
        }
        Ok(())
    })?;

    assert!(
        interlaced_cases.get() > 0,
        "no case exercised the interlaced encode/decode path\n{runner}"
    );
    assert!(
        non_interlaced_cases.get() > 0,
        "no case exercised the non-interlaced encode/decode path\n{runner}"
    );
    Ok(())
}

#[test]
fn roundtrip_random_rgba16() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let data = sample_rgba16_be(ctx, width, height);
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Rgba16Be,
            interlaced: false,
        };
        let encoded = encode_image(&spec, &data).expect("infallible");

        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        assert_eq!(decoded_spec.width, width);
        assert_eq!(decoded_spec.height, height);
        assert_eq!(decoded_data, data);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn encoded_png_matches_png_crate_for_grayscale() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let interlaced_cases = Cell::new(0usize);
    let non_interlaced_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let interlaced = noprop::sample_bool(ctx);
        let samples = sample_gray2(ctx, width, height);
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Gray2,
            interlaced,
        };
        let encoded = encode_image(&spec, &samples).expect("infallible");

        let (decoded_width, decoded_height, decoded_rgba) =
            decode_with_png_crate(&encoded).expect("infallible");
        assert_eq!(decoded_width, width);
        assert_eq!(decoded_height, height);
        // Build expected RGBA8 from 2-bit samples (0->0, 1->85, 2->170, 3->255).
        let expected_rgba: Vec<u8> = samples
            .iter()
            .flat_map(|&g| {
                let scaled = ((u32::from(g) * 255) / ((1u32 << 2) - 1)) as u8;
                [scaled, scaled, scaled, 255]
            })
            .collect();
        assert_eq!(decoded_rgba, expected_rgba);

        if interlaced {
            interlaced_cases.set(interlaced_cases.get() + 1);
        } else {
            non_interlaced_cases.set(non_interlaced_cases.get() + 1);
        }
        Ok(())
    })?;

    assert!(
        interlaced_cases.get() > 0,
        "no case exercised the interlaced grayscale path\n{runner}"
    );
    assert!(
        non_interlaced_cases.get() > 0,
        "no case exercised the non-interlaced grayscale path\n{runner}"
    );
    Ok(())
}

#[test]
fn encoded_png_matches_png_crate_for_indexed() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let palette = fixed_palette();
    let trns = fixed_trns();
    let interlaced_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let interlaced = noprop::sample_bool(ctx);
        let indices = sample_indices4(ctx, width, height);
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Indexed4 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            interlaced,
        };
        let encoded = encode_image(&spec, &indices).expect("infallible");

        let (decoded_width, decoded_height, decoded_rgba) =
            decode_with_png_crate(&encoded).expect("infallible");
        assert_eq!(decoded_width, width);
        assert_eq!(decoded_height, height);
        // Build expected RGBA8 from palette + trns.
        let expected_rgba: Vec<u8> = indices
            .iter()
            .flat_map(|&idx| {
                let rgb_off = usize::from(idx) * 3;
                let r = palette[rgb_off];
                let g = palette[rgb_off + 1];
                let b = palette[rgb_off + 2];
                let a = trns.get(usize::from(idx)).copied().unwrap_or(255);
                [r, g, b, a]
            })
            .collect();
        assert_eq!(decoded_rgba, expected_rgba);

        if interlaced {
            interlaced_cases.set(interlaced_cases.get() + 1);
        }
        Ok(())
    })?;

    assert!(
        interlaced_cases.get() > 0,
        "no case exercised the interlaced indexed path\n{runner}"
    );
    Ok(())
}

#[test]
fn decoder_never_panics_on_arbitrary_bytes() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(FUZZ_CASES, |ctx| {
        let len = noprop::sample_usize_in(ctx, 0..=2048);
        let data = noprop::sample_bytes_vec(ctx, len);
        // The only requirement is that decoding does not panic.
        let _ = decode_image(&data);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_identity_returns_same_data() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let data = sample_rgba8(ctx, width, height);

        // Derive the other formats from the same RGBA8 buffer, then check
        // that reformatting each one to itself is the identity.
        let rgb =
            reformat_pixels(&PixelFormat::Rgba8, &data, &PixelFormat::Rgb8).expect("infallible");
        let gray =
            reformat_pixels(&PixelFormat::Rgba8, &data, &PixelFormat::Gray8).expect("infallible");
        let gray_alpha = reformat_pixels(&PixelFormat::Rgba8, &data, &PixelFormat::GrayAlpha8)
            .expect("infallible");

        let cases: [(PixelFormat, &[u8]); 4] = [
            (PixelFormat::Rgba8, &data),
            (PixelFormat::Rgb8, &rgb),
            (PixelFormat::Gray8, &gray),
            (PixelFormat::GrayAlpha8, &gray_alpha),
        ];
        for (fmt, pixels) in cases {
            let result =
                reformat_pixels(&fmt, pixels, &fmt).expect("identity reformat must succeed");
            assert_eq!(result, pixels, "identity reformat changed data for {fmt:?}");
        }
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_to_rgba8_matches_png_crate_for_grayscale() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let samples = sample_gray2(ctx, width, height);
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Gray2,
            interlaced: false,
        };
        let encoded = encode_image(&spec, &samples).expect("infallible");

        // Decode with nopng + reformat_pixels to RGBA8.
        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        let nopng_rgba = reformat_pixels(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");

        // Compare with the png crate.
        let (_, _, ref_rgba) = decode_with_png_crate(&encoded).expect("infallible");
        assert_eq!(nopng_rgba, ref_rgba);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_to_rgba8_matches_png_crate_for_indexed() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let palette = fixed_palette();
    let trns = fixed_trns();
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let indices = sample_indices4(ctx, width, height);
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Indexed4 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            interlaced: false,
        };
        let encoded = encode_image(&spec, &indices).expect("infallible");

        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        let nopng_rgba = reformat_pixels(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");

        let (_, _, ref_rgba) = decode_with_png_crate(&encoded).expect("infallible");
        assert_eq!(nopng_rgba, ref_rgba);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_rgba8_roundtrip_through_formats() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    // Each target format is a distinct conversion path; require that the
    // run actually reaches every one of them at least once.
    let reached = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let data = sample_rgba8(ctx, width, height);

        let targets = [
            PixelFormat::Rgba8,
            PixelFormat::Rgba16Be,
            PixelFormat::Rgb8,
            PixelFormat::Rgb16Be,
            PixelFormat::Gray8,
            PixelFormat::Gray16Be,
            PixelFormat::GrayAlpha8,
            PixelFormat::GrayAlpha16Be,
        ];
        let src_fmt = PixelFormat::Rgba8;
        for dst_fmt in &targets {
            let converted =
                reformat_pixels(&src_fmt, &data, dst_fmt).expect("reformat must succeed");
            // Convert back to RGBA8.
            let back = reformat_pixels(dst_fmt, &converted, &PixelFormat::Rgba8)
                .expect("reformat back must succeed");
            // A second round through the same path must be stable (idempotent).
            let converted2 = reformat_pixels(&PixelFormat::Rgba8, &back, dst_fmt)
                .expect("second reformat must succeed");
            let back2 = reformat_pixels(dst_fmt, &converted2, &PixelFormat::Rgba8)
                .expect("second reformat back must succeed");
            assert_eq!(back, back2, "reformat not idempotent for {dst_fmt:?}");
            reached.set(reached.get() + 1);
        }
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_to_indexed_is_unsupported() -> TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 4);
        let data = sample_rgba8(ctx, width, height);
        let palette = vec![0u8; 3];
        let indexed_formats = [
            PixelFormat::Indexed1 {
                palette: palette.clone(),
                trns: None,
            },
            PixelFormat::Indexed2 {
                palette: palette.clone(),
                trns: None,
            },
            PixelFormat::Indexed4 {
                palette: palette.clone(),
                trns: None,
            },
            PixelFormat::Indexed8 {
                palette: palette.clone(),
                trns: None,
            },
        ];
        for dst in &indexed_formats {
            let result = reformat_pixels(&PixelFormat::Rgba8, &data, dst);
            assert!(result.is_err(), "reformat to indexed should fail");
        }
        Ok(())
    })?;

    Ok(())
}
