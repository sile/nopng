use std::assert_matches;

/// Builds an [`nopng::ImageSpec`] from literal dimensions.
fn image_spec(width: u32, height: u32, pixel_format: nopng::PixelFormat) -> nopng::ImageSpec {
    nopng::ImageSpec {
        width: core::num::NonZeroU32::new(width).expect("test dimensions are non-zero"),
        height: core::num::NonZeroU32::new(height).expect("test dimensions are non-zero"),
        pixel_format,
    }
}

fn rgba8(bytes: &[u8]) -> Vec<u8> {
    let (spec, data) = nopng::decode_image(bytes).expect("infallible");
    assert_eq!(data.len(), spec.data_len());
    nopng::reformat_pixels(&spec.pixel_format, &data, &nopng::PixelFormat::Rgba8)
        .expect("infallible")
}

#[test]
fn decodes_grayscale_png() {
    let (spec, _) =
        nopng::decode_image(include_bytes!("data/gray_filters.png")).expect("infallible");
    assert_eq!(spec.width.get(), 3);
    assert_eq!(spec.height.get(), 2);
    assert_eq!(
        rgba8(include_bytes!("data/gray_filters.png")),
        &[
            0, 0, 0, 255, 127, 127, 127, 255, 255, 255, 255, 255, 10, 10, 10, 255, 140, 140, 140,
            255, 200, 200, 200, 255,
        ]
    );
}

#[test]
fn reads_image_spec_from_ihdr() {
    let info = nopng::inspect(include_bytes!("data/gray16_interlaced.png")).expect("infallible");
    assert_eq!(info.width.get(), 5);
    assert_eq!(info.height.get(), 4);
    assert_matches!(
        info.pixel_format,
        nopng::PixelFormat::Gray16Be | nopng::PixelFormat::GrayAlpha16Be
    );
    assert_eq!(info.frames.get(), 1);
    assert!(!info.animated);
    assert!(info.interlaced);
}

#[test]
fn decodes_grayscale_alpha_png() {
    let (spec, _) =
        nopng::decode_image(include_bytes!("data/gray_alpha_avg.png")).expect("infallible");
    assert_eq!(spec.width.get(), 2);
    assert_eq!(spec.height.get(), 2);
    assert_eq!(
        rgba8(include_bytes!("data/gray_alpha_avg.png")),
        &[
            20, 20, 20, 255, 180, 180, 180, 64, 100, 100, 100, 128, 220, 220, 220, 200,
        ]
    );
}

#[test]
fn decodes_rgb_png() {
    let (spec, _) = nopng::decode_image(include_bytes!("data/rgb_sub_up.png")).expect("infallible");
    assert_eq!(spec.width.get(), 2);
    assert_eq!(spec.height.get(), 2);
    assert_eq!(
        rgba8(include_bytes!("data/rgb_sub_up.png")),
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ]
    );
}

#[test]
fn decodes_rgba_png_with_split_idat() {
    let (spec, _) =
        nopng::decode_image(include_bytes!("data/rgba_paeth_split_idat.png")).expect("infallible");
    assert_eq!(spec.width.get(), 2);
    assert_eq!(spec.height.get(), 2);
    assert_eq!(
        rgba8(include_bytes!("data/rgba_paeth_split_idat.png")),
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 64,
        ]
    );
}

#[test]
fn decodes_1bit_grayscale_png() {
    let (spec, _) =
        nopng::decode_image(include_bytes!("data/gray_1bit_filters.png")).expect("infallible");
    assert_eq!(spec.width.get(), 5);
    assert_eq!(spec.height.get(), 2);
    assert_matches!(spec.pixel_format, nopng::PixelFormat::Gray1);
    assert_eq!(
        rgba8(include_bytes!("data/gray_1bit_filters.png")),
        &[
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ]
    );
}

#[test]
fn decodes_2bit_grayscale_with_trns() {
    assert_eq!(
        rgba8(include_bytes!("data/gray_2bit_trns.png")),
        &[
            0, 0, 0, 255, 85, 85, 85, 0, 170, 170, 170, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 170, 170, 170, 255, 85, 85, 85, 0, 0, 0, 0, 255,
        ]
    );
}

#[test]
fn decodes_4bit_palette_with_trns() {
    assert_eq!(
        rgba8(include_bytes!("data/palette_4bit_trns.png")),
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 0, 255, 255, 255, 0, 255, 0, 0,
            255, 0, 0, 255, 0, 128, 255, 0, 0, 255,
        ]
    );
}

#[test]
fn decodes_16bit_rgba() {
    let (spec, _) = nopng::decode_image(include_bytes!("data/rgba16.png")).expect("infallible");
    assert_eq!(spec.width.get(), 2);
    assert_eq!(spec.height.get(), 1);
    assert_eq!(
        rgba8(include_bytes!("data/rgba16.png")),
        &[255, 128, 0, 255, 18, 171, 255, 1,]
    );
}

#[test]
fn decodes_rgba_interlaced() {
    let rgba = rgba8(include_bytes!("data/rgba_interlaced.png"));
    // Re-encode as non-interlaced and verify roundtrip.
    let spec2 = image_spec(4, 4, nopng::PixelFormat::Rgba8);
    let bytes = nopng::encode_image(&spec2, &rgba).expect("infallible");
    assert_eq!(rgba8(&bytes), rgba);
}

#[test]
fn decodes_palette_interlaced() {
    let (spec, _) =
        nopng::decode_image(include_bytes!("data/palette_interlaced.png")).expect("infallible");
    assert_matches!(
        spec.pixel_format,
        nopng::PixelFormat::Indexed1 { .. }
            | nopng::PixelFormat::Indexed2 { .. }
            | nopng::PixelFormat::Indexed4 { .. }
            | nopng::PixelFormat::Indexed8 { .. }
    );
    // Verify we can roundtrip through RGBA.
    let rgba = rgba8(include_bytes!("data/palette_interlaced.png"));
    assert!(!rgba.is_empty());
}

#[test]
fn decodes_gray16_interlaced() {
    let (spec, _) =
        nopng::decode_image(include_bytes!("data/gray16_interlaced.png")).expect("infallible");
    assert_eq!(spec.width.get(), 5);
    assert_eq!(spec.height.get(), 4);
    assert_matches!(
        spec.pixel_format,
        nopng::PixelFormat::Gray16Be | nopng::PixelFormat::GrayAlpha16Be
    );
    let rgba = rgba8(include_bytes!("data/gray16_interlaced.png"));
    assert_eq!(rgba.len(), 5 * 4 * 4);
}

#[test]
fn decodes_interlaced_low_bit_images_with_empty_passes() {
    // Regression test: Adam7 passes can be empty for small images
    // (e.g. 2x2 has passes with width 0 and/or height 0). The packed
    // low-bit grayscale and indexed decoders used to call
    // `chunks_exact(0)` for those passes and panic with "chunk size must
    // be non-zero"; they must instead decode cleanly.
    for (width, height) in [(1, 1), (1, 8), (8, 1), (2, 2), (3, 3)] {
        let gray = image_spec(width, height, nopng::PixelFormat::Gray2);
        let samples: Vec<u8> = (0..width * height).map(|i| (i % 4) as u8).collect();
        let encoded = nopng::encode_image_interlaced(&gray, &samples).expect("infallible");
        let (decoded_spec, decoded) = nopng::decode_image(&encoded).expect("infallible");
        assert_eq!(decoded_spec.width, gray.width);
        assert_eq!(
            decoded, samples,
            "gray2 interlace mismatch for {width}x{height}"
        );

        let indexed = image_spec(
            width,
            height,
            nopng::PixelFormat::Indexed4 {
                palette: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
                trns: None,
            },
        );
        let indices: Vec<u8> = (0..width * height).map(|i| (i % 4) as u8).collect();
        let encoded = nopng::encode_image_interlaced(&indexed, &indices).expect("infallible");
        let (decoded_spec, decoded) = nopng::decode_image(&encoded).expect("infallible");
        assert_eq!(decoded_spec.width, indexed.width);
        assert_eq!(
            decoded, indices,
            "indexed4 interlace mismatch for {width}x{height}"
        );
    }
}

#[test]
fn rejects_crc_mismatch() {
    let mut bytes = include_bytes!("data/gray_filters.png").to_vec();
    let index = bytes.len() - 1;
    bytes[index] ^= 0x01;
    let error = nopng::decode_image(&bytes).expect_err("infallible");
    assert!(
        matches!(error, nopng::Error::InvalidData(message) if message.contains("CRC mismatch"))
    );
}

#[test]
fn rejects_missing_plte_for_palette_image() {
    let mut bytes = include_bytes!("data/palette_4bit_trns.png").to_vec();
    remove_chunk(&mut bytes, b"PLTE");
    remove_chunk(&mut bytes, b"tRNS");
    let error = nopng::decode_image(&bytes).expect_err("infallible");
    assert!(
        matches!(error, nopng::Error::InvalidData(message) if message.contains("missing PLTE"))
    );
}

fn remove_chunk(bytes: &mut Vec<u8>, chunk_type: &[u8; 4]) {
    let chunks = collect_chunks(bytes);
    let kept = chunks
        .into_iter()
        .filter(|chunk| &chunk.chunk_type != chunk_type)
        .collect::<Vec<_>>();
    *bytes = rebuild_png(&kept);
}

struct Chunk {
    chunk_type: [u8; 4],
    data: Vec<u8>,
}

fn collect_chunks(bytes: &[u8]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut offset = 8;
    while offset + 12 <= bytes.len() {
        let length = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("bug: chunk length must be 4 bytes"),
        ) as usize;
        offset += 4;
        let chunk_type = bytes[offset..offset + 4]
            .try_into()
            .expect("bug: chunk type must be 4 bytes");
        offset += 4;
        let data = bytes[offset..offset + length].to_vec();
        offset += length + 4;
        chunks.push(Chunk { chunk_type, data });
        if &chunk_type == b"IEND" {
            break;
        }
    }
    chunks
}

fn rebuild_png(chunks: &[Chunk]) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    for chunk in chunks {
        bytes.extend_from_slice(&(chunk.data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&chunk.chunk_type);
        bytes.extend_from_slice(&chunk.data);
        let mut crc_data = Vec::new();
        crc_data.extend_from_slice(&chunk.chunk_type);
        crc_data.extend_from_slice(&chunk.data);
        bytes.extend_from_slice(&crc32(&crc_data).to_be_bytes());
    }
    bytes
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & 0xEDB8_8320;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}
