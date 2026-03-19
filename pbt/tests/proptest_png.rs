use std::io::Cursor;

use nopng::{ImageSpec, PixelFormat, decode_image, encode_image, reformat_pixels};
use proptest::prelude::*;

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
            .chunks_exact(3)
            .flat_map(|chunk| [chunk[0], chunk[1], chunk[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|chunk| [chunk[0], chunk[0], chunk[0], chunk[1]])
            .collect(),
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Indexed => unreachable!("indexed data should be expanded by png crate"),
    }
}

fn rgba_image_strategy(
    max_width: u32,
    max_height: u32,
) -> impl Strategy<Value = (u32, u32, Vec<u8>)> {
    (1u32..=max_width, 1u32..=max_height).prop_flat_map(|(width, height)| {
        let len = (width * height * 4) as usize;
        (
            Just(width),
            Just(height),
            proptest::collection::vec(any::<u8>(), len),
        )
    })
}

fn grayscale_levels_strategy() -> impl Strategy<Value = (u32, u32, Vec<u8>)> {
    // 2-bit grayscale: sample values 0, 1, 2, 3.
    let levels = prop::sample::select(vec![0u8, 1, 2, 3]);
    (1u32..=8, 1u32..=8).prop_flat_map(move |(width, height)| {
        let pixels = (width * height) as usize;
        (
            Just(width),
            Just(height),
            proptest::collection::vec(levels.clone(), pixels),
        )
    })
}

fn rgba16_image_strategy(
    max_width: u32,
    max_height: u32,
) -> impl Strategy<Value = (u32, u32, Vec<u8>)> {
    (1u32..=max_width, 1u32..=max_height).prop_flat_map(|(width, height)| {
        // 8 bytes per pixel (RGBA16Be)
        let len = (width * height * 8) as usize;
        (
            Just(width),
            Just(height),
            proptest::collection::vec(any::<u8>(), len),
        )
    })
}

fn indexed_image_strategy() -> impl Strategy<Value = (u32, u32, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let palette = vec![255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0];
    let trns = vec![255u8, 128, 255, 64];
    (1u32..=8, 1u32..=8).prop_flat_map(move |(width, height)| {
        let pixels = (width * height) as usize;
        (
            Just(width),
            Just(height),
            proptest::collection::vec(0u8..4, pixels),
            Just(palette.clone()),
            Just(trns.clone()),
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 100,
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    #[test]
    fn roundtrip_random_rgba((width, height, data) in rgba_image_strategy(8, 8)) {
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Rgba8,
            interlaced: false,
        };
        let encoded = encode_image(&spec, &data).expect("infallible");

        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        prop_assert_eq!(decoded_spec.width, width);
        prop_assert_eq!(decoded_spec.height, height);
        prop_assert_eq!(decoded_data, data);
    }

    #[test]
    fn encoded_png_matches_png_crate_for_grayscale((width, height, samples) in grayscale_levels_strategy(), interlaced in any::<bool>()) {
        // Encode as grayscale (samples are gray levels that fit in 2-bit).
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Gray2,
            interlaced,
        };
        let encoded = encode_image(&spec, &samples).expect("infallible");

        // Decode with png crate and compare RGBA8 expansion.
        let (decoded_width, decoded_height, decoded_rgba) =
            decode_with_png_crate(&encoded).expect("infallible");
        prop_assert_eq!(decoded_width, width);
        prop_assert_eq!(decoded_height, height);
        // Build expected RGBA8 from 2-bit samples (0→0, 1→85, 2→170, 3→255).
        let expected_rgba: Vec<u8> = samples.iter().flat_map(|&g| {
            let scaled = ((u32::from(g) * 255) / ((1u32 << 2) - 1)) as u8;
            [scaled, scaled, scaled, 255]
        }).collect();
        prop_assert_eq!(decoded_rgba, expected_rgba);
    }

    #[test]
    fn encoded_png_matches_png_crate_for_indexed((width, height, indices, palette, trns) in indexed_image_strategy(), interlaced in any::<bool>()) {
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
        prop_assert_eq!(decoded_width, width);
        prop_assert_eq!(decoded_height, height);
        // Build expected RGBA8 from palette + trns.
        let expected_rgba: Vec<u8> = indices.iter().flat_map(|&idx| {
            let rgb_off = usize::from(idx) * 3;
            let r = palette[rgb_off];
            let g = palette[rgb_off + 1];
            let b = palette[rgb_off + 2];
            let a = trns.get(usize::from(idx)).copied().unwrap_or(255);
            [r, g, b, a]
        }).collect();
        prop_assert_eq!(decoded_rgba, expected_rgba);
    }

    #[test]
    fn roundtrip_random_rgba_interlaced((width, height, data) in rgba_image_strategy(8, 8)) {
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Rgba8,
            interlaced: true,
        };
        let encoded = encode_image(&spec, &data).expect("infallible");

        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        prop_assert_eq!(decoded_spec.width, width);
        prop_assert_eq!(decoded_spec.height, height);
        prop_assert_eq!(decoded_data, data);
    }

    #[test]
    fn roundtrip_random_rgba16((width, height, data) in rgba16_image_strategy(8, 8)) {
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Rgba16Be,
            interlaced: false,
        };
        let encoded = encode_image(&spec, &data).expect("infallible");

        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        prop_assert_eq!(decoded_spec.width, width);
        prop_assert_eq!(decoded_spec.height, height);
        prop_assert_eq!(decoded_data, data);
    }

    #[test]
    fn decoder_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = decode_image(&data);
    }

    #[test]
    fn reformat_identity_returns_same_data((_width, _height, data) in rgba_image_strategy(8, 8)) {
        let formats: Vec<(PixelFormat, Vec<u8>)> = vec![
            (PixelFormat::Rgba8, data.clone()),
            (PixelFormat::Rgb8, {
                let rgb = reformat_pixels(&PixelFormat::Rgba8, &data, &PixelFormat::Rgb8).unwrap();
                rgb
            }),
            (PixelFormat::Gray8, {
                let g = reformat_pixels(&PixelFormat::Rgba8, &data, &PixelFormat::Gray8).unwrap();
                g
            }),
            (PixelFormat::GrayAlpha8, {
                let ga = reformat_pixels(&PixelFormat::Rgba8, &data, &PixelFormat::GrayAlpha8).unwrap();
                ga
            }),
        ];
        for (fmt, pixels) in &formats {
            let result = reformat_pixels(fmt, pixels, fmt).expect("identity reformat must succeed");
            prop_assert_eq!(&result, pixels, "identity reformat changed data for {:?}", fmt);
        }
    }

    #[test]
    fn reformat_to_rgba8_matches_png_crate_for_grayscale((width, height, samples) in grayscale_levels_strategy()) {
        let spec = ImageSpec {
            width,
            height,
            pixel_format: PixelFormat::Gray2,
            interlaced: false,
        };
        let encoded = encode_image(&spec, &samples).expect("infallible");

        // Decode with nopng + reformat_pixels to RGBA8.
        let (decoded_spec, decoded_data) = decode_image(&encoded).expect("infallible");
        let nopng_rgba = reformat_pixels(&decoded_spec.pixel_format, &decoded_data, &PixelFormat::Rgba8).expect("infallible");

        // Compare with png crate.
        let (_, _, ref_rgba) = decode_with_png_crate(&encoded).expect("infallible");
        prop_assert_eq!(nopng_rgba, ref_rgba);
    }

    #[test]
    fn reformat_to_rgba8_matches_png_crate_for_indexed((width, height, indices, palette, trns) in indexed_image_strategy()) {
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
        let nopng_rgba = reformat_pixels(&decoded_spec.pixel_format, &decoded_data, &PixelFormat::Rgba8).expect("infallible");

        let (_, _, ref_rgba) = decode_with_png_crate(&encoded).expect("infallible");
        prop_assert_eq!(nopng_rgba, ref_rgba);
    }

    #[test]
    fn reformat_rgba8_roundtrip_through_formats((_width, _height, data) in rgba_image_strategy(8, 8)) {
        // RGBA8 → fmt → RGBA8 should be stable (though not necessarily identical
        // to the original due to lossy conversions like dropping alpha).
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
            let converted = reformat_pixels(&src_fmt, &data, dst_fmt).expect("reformat must succeed");
            // Convert back to RGBA8.
            let back = reformat_pixels(dst_fmt, &converted, &PixelFormat::Rgba8).expect("reformat back must succeed");
            // A second round through the same path must be stable (idempotent).
            let converted2 = reformat_pixels(&PixelFormat::Rgba8, &back, dst_fmt).expect("second reformat must succeed");
            let back2 = reformat_pixels(dst_fmt, &converted2, &PixelFormat::Rgba8).expect("second reformat back must succeed");
            prop_assert_eq!(&back, &back2, "reformat not idempotent for {:?}", dst_fmt);
        }
    }

    #[test]
    fn reformat_to_indexed_is_unsupported((_width, _height, data) in rgba_image_strategy(4, 4)) {
        let palette = vec![0u8; 3];
        let indexed_formats = [
            PixelFormat::Indexed1 { palette: palette.clone(), trns: None },
            PixelFormat::Indexed2 { palette: palette.clone(), trns: None },
            PixelFormat::Indexed4 { palette: palette.clone(), trns: None },
            PixelFormat::Indexed8 { palette: palette.clone(), trns: None },
        ];
        for dst in &indexed_formats {
            let result = reformat_pixels(&PixelFormat::Rgba8, &data, dst);
            prop_assert!(result.is_err(), "reformat to indexed should fail");
        }
    }
}
