use std::io::Cursor;

use nopng::{PngColorMode, PngEncodeOptions, PngRgbaImage};
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
    let levels = prop::sample::select(vec![0u8, 85, 170, 255]);
    (1u32..=8, 1u32..=8).prop_flat_map(move |(width, height)| {
        let pixels = (width * height) as usize;
        (
            Just(width),
            Just(height),
            proptest::collection::vec((levels.clone(), Just(255u8)), pixels).prop_map(|values| {
                let mut rgba = Vec::with_capacity(values.len() * 4);
                for (gray, alpha) in values {
                    rgba.extend_from_slice(&[gray, gray, gray, alpha]);
                }
                rgba
            }),
        )
    })
}

fn indexed_image_strategy() -> impl Strategy<Value = (u32, u32, Vec<u8>)> {
    let palette = vec![
        [255, 0, 0, 255],
        [0, 255, 0, 128],
        [0, 0, 255, 255],
        [255, 255, 0, 64],
    ];
    (1u32..=8, 1u32..=8).prop_flat_map(move |(width, height)| {
        let pixels = (width * height) as usize;
        (
            Just(width),
            Just(height),
            proptest::collection::vec(0usize..palette.len(), pixels).prop_map({
                let palette = palette.clone();
                move |indices| {
                    let mut rgba = Vec::with_capacity(indices.len() * 4);
                    for index in indices {
                        rgba.extend_from_slice(&palette[index]);
                    }
                    rgba
                }
            }),
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
    fn roundtrip_random_rgba((width, height, rgba) in rgba_image_strategy(8, 8)) {
        let image = PngRgbaImage::new(width, height, rgba.clone()).unwrap();
        let mut encoded = Vec::new();
        image.write_to(&mut encoded).unwrap();

        let decoded = PngRgbaImage::from_bytes(&encoded).unwrap();
        prop_assert_eq!(decoded.width(), width);
        prop_assert_eq!(decoded.height(), height);
        prop_assert_eq!(decoded.data(), rgba.as_slice());
    }

    #[test]
    fn encoded_png_matches_png_crate_for_grayscale((width, height, rgba) in grayscale_levels_strategy(), interlaced in any::<bool>()) {
        let image = PngRgbaImage::new(width, height, rgba.clone()).unwrap();
        let mut encoded = Vec::new();
        image.write_to_with_options(&mut encoded, PngEncodeOptions {
            color_mode: PngColorMode::Grayscale,
            interlaced,
        }).unwrap();

        let (decoded_width, decoded_height, decoded_rgba) = decode_with_png_crate(&encoded).unwrap();
        prop_assert_eq!(decoded_width, width);
        prop_assert_eq!(decoded_height, height);
        prop_assert_eq!(decoded_rgba, rgba);
    }

    #[test]
    fn encoded_png_matches_png_crate_for_indexed((width, height, rgba) in indexed_image_strategy(), interlaced in any::<bool>()) {
        let image = PngRgbaImage::new(width, height, rgba.clone()).unwrap();
        let mut encoded = Vec::new();
        image.write_to_with_options(&mut encoded, PngEncodeOptions {
            color_mode: PngColorMode::Indexed,
            interlaced,
        }).unwrap();

        let (decoded_width, decoded_height, decoded_rgba) = decode_with_png_crate(&encoded).unwrap();
        prop_assert_eq!(decoded_width, width);
        prop_assert_eq!(decoded_height, height);
        prop_assert_eq!(decoded_rgba, rgba);
    }

    #[test]
    fn decoder_never_panics_on_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = PngRgbaImage::from_bytes(&data);
    }
}
