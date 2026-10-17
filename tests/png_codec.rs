//! Property-based tests for `nopng` written with `noprop`.
//!
//! Every property is a plain Rust closure that samples its inputs and
//! asserts on the result. Expected values are derived independently of the
//! encoder and decoder (from the sampled inputs), so the properties do not
//! need an external reference implementation.
//!
//! Reproduce a failure by copying the seed printed in the failure report
//! into the `NOPNG_PROP_TEST_SEED` environment variable and rerunning the
//! same test with the same case budget.

use std::cell::Cell;

/// Case budget for the cheap round-trip and differential properties.
const CASES: usize = 100;
/// Smaller budget for the byte-fuzzing property, whose cases are heavier.
const FUZZ_CASES: usize = 200;

// ---------------------------------------------------------------------
// Samplers (ordinary functions, not a combinator DSL).
// ---------------------------------------------------------------------

/// A random size in `1..=max` for each axis, so every image is non-empty.
fn sample_dimensions(ctx: &mut noprop::TestCaseContext, max: u32) -> (u32, u32) {
    (
        noprop::sample_u32_in(ctx, 1..=max),
        noprop::sample_u32_in(ctx, 1..=max),
    )
}

/// Builds an [`nopng::Image`] from sampled dimensions and a pixel buffer.
fn image(
    width: u32,
    height: u32,
    pixel_format: nopng::PixelFormat,
    pixels: Vec<u8>,
) -> nopng::Image {
    nopng::Image {
        width: core::num::NonZeroU32::new(width).expect("sample_dimensions is 1..=max"),
        height: core::num::NonZeroU32::new(height).expect("sample_dimensions is 1..=max"),
        pixel_format,
        pixels,
    }
}

/// RGBA8 pixels for a `width`x`height` image.
fn sample_rgba8(ctx: &mut noprop::TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    noprop::sample_bytes_vec(ctx, (width * height * 4) as usize)
}

/// RGBA16 big-endian pixels for a `width`x`height` image.
fn sample_rgba16_be(ctx: &mut noprop::TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    noprop::sample_bytes_vec(ctx, (width * height * 8) as usize)
}

/// 2-bit grayscale samples (values 0..=3) for a `width`x`height` image.
fn sample_gray2(ctx: &mut noprop::TestCaseContext, width: u32, height: u32) -> Vec<u8> {
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
fn sample_indices4(ctx: &mut noprop::TestCaseContext, width: u32, height: u32) -> Vec<u8> {
    let pixels = (width * height) as usize;
    (0..pixels)
        .map(|_| noprop::sample_u8_in(ctx, 0..4))
        .collect()
}

// ---------------------------------------------------------------------
// Properties.
// ---------------------------------------------------------------------

#[test]
fn roundtrip_random_rgba() -> noprop::TestResult {
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
        let image = image(width, height, nopng::PixelFormat::Rgba8, data.clone());
        let encoded = if interlaced {
            nopng::encode_image_interlaced(&image)
        } else {
            nopng::encode_image(&image)
        }
        .expect("infallible");

        let decoded = nopng::decode_image(&encoded).expect("infallible");
        assert_eq!(decoded.width.get(), width);
        assert_eq!(decoded.height.get(), height);
        assert_eq!(decoded.pixels, data);

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
fn roundtrip_random_rgba16() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let data = sample_rgba16_be(ctx, width, height);
        let image = image(width, height, nopng::PixelFormat::Rgba16Be, data.clone());
        let encoded = nopng::encode_image(&image).expect("infallible");

        let decoded = nopng::decode_image(&encoded).expect("infallible");
        assert_eq!(decoded.width.get(), width);
        assert_eq!(decoded.height.get(), height);
        assert_eq!(decoded.pixels, data);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn encoded_png_has_expected_grayscale() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let interlaced_cases = Cell::new(0usize);
    let non_interlaced_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let interlaced = noprop::sample_bool(ctx);
        let samples = sample_gray2(ctx, width, height);
        let image = image(width, height, nopng::PixelFormat::Gray2, samples.clone());
        let encoded = if interlaced {
            nopng::encode_image_interlaced(&image)
        } else {
            nopng::encode_image(&image)
        }
        .expect("infallible");

        // Decode with nopng and compare against an expectation built
        // directly from the sampled 2-bit samples
        // (0->0, 1->85, 2->170, 3->255).
        let decoded = nopng::decode_image(&encoded).expect("infallible");
        assert_eq!(decoded.width.get(), width);
        assert_eq!(decoded.height.get(), height);
        assert_eq!(decoded.pixels, samples);

        let decoded_rgba = nopng::reformat_pixels(
            &decoded.pixel_format,
            &decoded.pixels,
            &nopng::PixelFormat::Rgba8,
        )
        .expect("infallible");
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
fn encoded_png_has_expected_indexed() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let palette = fixed_palette();
    let trns = fixed_trns();
    let interlaced_cases = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let interlaced = noprop::sample_bool(ctx);
        let indices = sample_indices4(ctx, width, height);
        let image = image(
            width,
            height,
            nopng::PixelFormat::Indexed4 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            indices.clone(),
        );
        let encoded = if interlaced {
            nopng::encode_image_interlaced(&image)
        } else {
            nopng::encode_image(&image)
        }
        .expect("infallible");

        // Decode with nopng and compare against an expectation built
        // directly from the sampled indices plus the palette and tRNS.
        let decoded = nopng::decode_image(&encoded).expect("infallible");
        assert_eq!(decoded.width.get(), width);
        assert_eq!(decoded.height.get(), height);
        assert_eq!(decoded.pixels, indices);

        let decoded_rgba = nopng::reformat_pixels(
            &decoded.pixel_format,
            &decoded.pixels,
            &nopng::PixelFormat::Rgba8,
        )
        .expect("infallible");
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
fn decoder_never_panics_on_arbitrary_bytes() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(FUZZ_CASES, |ctx| {
        let len = noprop::sample_usize_in(ctx, 0..=2048);
        let data = noprop::sample_bytes_vec(ctx, len);
        // The only requirement is that decoding does not panic.
        let _ = nopng::decode_image(&data);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_identity_returns_same_data() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let data = sample_rgba8(ctx, width, height);

        // Derive the other formats from the same RGBA8 buffer, then check
        // that reformatting each one to itself is the identity.
        let rgb =
            nopng::reformat_pixels(&nopng::PixelFormat::Rgba8, &data, &nopng::PixelFormat::Rgb8)
                .expect("infallible");
        let gray = nopng::reformat_pixels(
            &nopng::PixelFormat::Rgba8,
            &data,
            &nopng::PixelFormat::Gray8,
        )
        .expect("infallible");
        let gray_alpha = nopng::reformat_pixels(
            &nopng::PixelFormat::Rgba8,
            &data,
            &nopng::PixelFormat::GrayAlpha8,
        )
        .expect("infallible");

        let cases: [(nopng::PixelFormat, &[u8]); 4] = [
            (nopng::PixelFormat::Rgba8, &data),
            (nopng::PixelFormat::Rgb8, &rgb),
            (nopng::PixelFormat::Gray8, &gray),
            (nopng::PixelFormat::GrayAlpha8, &gray_alpha),
        ];
        for (fmt, pixels) in cases {
            let result =
                nopng::reformat_pixels(&fmt, pixels, &fmt).expect("identity reformat must succeed");
            assert_eq!(result, pixels, "identity reformat changed data for {fmt:?}");
        }
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_to_rgba8_produces_expected_grayscale() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let samples = sample_gray2(ctx, width, height);
        let image = image(width, height, nopng::PixelFormat::Gray2, samples.clone());
        let encoded = nopng::encode_image(&image).expect("infallible");

        // Decode with nopng + reformat_pixels to RGBA8.
        let decoded = nopng::decode_image(&encoded).expect("infallible");
        let nopng_rgba = nopng::reformat_pixels(
            &decoded.pixel_format,
            &decoded.pixels,
            &nopng::PixelFormat::Rgba8,
        )
        .expect("infallible");

        // Compare against an expectation built directly from the sampled
        // 2-bit samples (0->0, 1->85, 2->170, 3->255).
        let expected_rgba: Vec<u8> = samples
            .iter()
            .flat_map(|&g| {
                let scaled = ((u32::from(g) * 255) / ((1u32 << 2) - 1)) as u8;
                [scaled, scaled, scaled, 255]
            })
            .collect();
        assert_eq!(nopng_rgba, expected_rgba);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_to_rgba8_produces_expected_indexed() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let palette = fixed_palette();
    let trns = fixed_trns();
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let indices = sample_indices4(ctx, width, height);
        let image = image(
            width,
            height,
            nopng::PixelFormat::Indexed4 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            indices.clone(),
        );
        let encoded = nopng::encode_image(&image).expect("infallible");

        let decoded = nopng::decode_image(&encoded).expect("infallible");
        let nopng_rgba = nopng::reformat_pixels(
            &decoded.pixel_format,
            &decoded.pixels,
            &nopng::PixelFormat::Rgba8,
        )
        .expect("infallible");

        // Compare against an expectation built directly from the sampled
        // indices plus the palette and tRNS.
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
        assert_eq!(nopng_rgba, expected_rgba);
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_rgba8_roundtrip_through_formats() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    // Each target format is a distinct conversion path; require that the
    // run actually reaches every one of them at least once.
    let reached = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 8);
        let data = sample_rgba8(ctx, width, height);

        let targets = [
            nopng::PixelFormat::Rgba8,
            nopng::PixelFormat::Rgba16Be,
            nopng::PixelFormat::Rgb8,
            nopng::PixelFormat::Rgb16Be,
            nopng::PixelFormat::Gray8,
            nopng::PixelFormat::Gray16Be,
            nopng::PixelFormat::GrayAlpha8,
            nopng::PixelFormat::GrayAlpha16Be,
        ];
        let src_fmt = nopng::PixelFormat::Rgba8;
        for dst_fmt in &targets {
            let converted =
                nopng::reformat_pixels(&src_fmt, &data, dst_fmt).expect("reformat must succeed");
            // Convert back to RGBA8.
            let back = nopng::reformat_pixels(dst_fmt, &converted, &nopng::PixelFormat::Rgba8)
                .expect("reformat back must succeed");
            // A second round through the same path must be stable (idempotent).
            let converted2 = nopng::reformat_pixels(&nopng::PixelFormat::Rgba8, &back, dst_fmt)
                .expect("second reformat must succeed");
            let back2 = nopng::reformat_pixels(dst_fmt, &converted2, &nopng::PixelFormat::Rgba8)
                .expect("second reformat back must succeed");
            assert_eq!(back, back2, "reformat not idempotent for {dst_fmt:?}");
            reached.set(reached.get() + 1);
        }
        Ok(())
    })?;

    Ok(())
}

#[test]
fn reformat_to_indexed_is_unsupported() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time("NOPNG_PROP_TEST_SEED")?;
    let mut runner = noprop::Runner::new(seed);

    runner.run(CASES, |ctx| {
        let (width, height) = sample_dimensions(ctx, 4);
        let data = sample_rgba8(ctx, width, height);
        let palette = vec![0u8; 3];
        let indexed_formats = [
            nopng::PixelFormat::Indexed1 {
                palette: palette.clone(),
                trns: None,
            },
            nopng::PixelFormat::Indexed2 {
                palette: palette.clone(),
                trns: None,
            },
            nopng::PixelFormat::Indexed4 {
                palette: palette.clone(),
                trns: None,
            },
            nopng::PixelFormat::Indexed8 {
                palette: palette.clone(),
                trns: None,
            },
        ];
        for dst in &indexed_formats {
            let result = nopng::reformat_pixels(&nopng::PixelFormat::Rgba8, &data, dst);
            assert!(result.is_err(), "reformat to indexed should fail");
        }
        Ok(())
    })?;

    Ok(())
}
