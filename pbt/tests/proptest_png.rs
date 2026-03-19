use std::io::Cursor;

use nopng::{ImageSpec, PixelFormat, decode_image, encode_image};
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

        let (decoded_spec, decoded_data) = decode_image(&encoded, Some(&PixelFormat::Rgba8)).expect("infallible");
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

        let (decoded_spec, decoded_data) = decode_image(&encoded, Some(&PixelFormat::Rgba8)).expect("infallible");
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

        let (decoded_spec, decoded_data) = decode_image(&encoded, None).expect("infallible");
        prop_assert_eq!(decoded_spec.width, width);
        prop_assert_eq!(decoded_spec.height, height);
        prop_assert_eq!(decoded_data, data);
    }

    #[test]
    fn decoder_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = decode_image(&data, None);
    }
}
