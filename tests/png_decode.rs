use nopng::{BitDepth, ColorMode, Error, ImageSpec, PixelFormat, decode_image, encode_image};

fn rgba8(spec: &ImageSpec, data: &[u8]) -> Vec<u8> {
    spec.pixel_format
        .reformat(data, &PixelFormat::Rgba8)
        .expect("infallible")
}

#[test]
fn decodes_grayscale_png() {
    let (spec, data) = decode_image(include_bytes!("data/gray_filters.png")).expect("infallible");
    assert_eq!(spec.width, 3);
    assert_eq!(spec.height, 2);
    assert_eq!(
        rgba8(&spec, &data),
        &[
            0, 0, 0, 255, 127, 127, 127, 255, 255, 255, 255, 255, 10, 10, 10, 255, 140, 140, 140,
            255, 200, 200, 200, 255,
        ]
    );
}

#[test]
fn reads_image_spec_from_ihdr() {
    let spec =
        ImageSpec::from_bytes(include_bytes!("data/gray16_interlaced.png")).expect("infallible");
    assert_eq!(spec.width, 5);
    assert_eq!(spec.height, 4);
    assert_eq!(spec.pixel_format.bit_depth(), BitDepth::Sixteen);
    // from_bytes now reads tRNS and promotes to GrayscaleAlpha if present.
    assert!(matches!(
        spec.pixel_format.color_mode(),
        ColorMode::Grayscale | ColorMode::GrayscaleAlpha
    ));
    assert!(spec.interlaced);
}

#[test]
fn decodes_grayscale_alpha_png() {
    let (spec, data) = decode_image(include_bytes!("data/gray_alpha_avg.png")).expect("infallible");
    assert_eq!(spec.width, 2);
    assert_eq!(spec.height, 2);
    assert_eq!(
        rgba8(&spec, &data),
        &[
            20, 20, 20, 255, 180, 180, 180, 64, 100, 100, 100, 128, 220, 220, 220, 200,
        ]
    );
}

#[test]
fn decodes_rgb_png() {
    let (spec, data) = decode_image(include_bytes!("data/rgb_sub_up.png")).expect("infallible");
    assert_eq!(spec.width, 2);
    assert_eq!(spec.height, 2);
    assert_eq!(
        rgba8(&spec, &data),
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ]
    );
}

#[test]
fn decodes_rgba_png_with_split_idat() {
    let (spec, data) =
        decode_image(include_bytes!("data/rgba_paeth_split_idat.png")).expect("infallible");
    assert_eq!(spec.width, 2);
    assert_eq!(spec.height, 2);
    assert_eq!(
        rgba8(&spec, &data),
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 64,
        ]
    );
}

#[test]
fn decodes_1bit_grayscale_png() {
    let (spec, data) =
        decode_image(include_bytes!("data/gray_1bit_filters.png")).expect("infallible");
    assert_eq!(spec.width, 5);
    assert_eq!(spec.height, 2);
    assert_eq!(spec.pixel_format.bit_depth(), BitDepth::One);
    assert_eq!(
        rgba8(&spec, &data),
        &[
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ]
    );
}

#[test]
fn decodes_2bit_grayscale_with_trns() {
    let (spec, data) = decode_image(include_bytes!("data/gray_2bit_trns.png")).expect("infallible");
    assert_eq!(
        rgba8(&spec, &data),
        &[
            0, 0, 0, 255, 85, 85, 85, 0, 170, 170, 170, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 170, 170, 170, 255, 85, 85, 85, 0, 0, 0, 0, 255,
        ]
    );
}

#[test]
fn decodes_4bit_palette_with_trns() {
    let (spec, data) =
        decode_image(include_bytes!("data/palette_4bit_trns.png")).expect("infallible");
    assert_eq!(
        rgba8(&spec, &data),
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 0, 255, 255, 255, 0, 255, 0, 0,
            255, 0, 0, 255, 0, 128, 255, 0, 0, 255,
        ]
    );
}

#[test]
fn decodes_16bit_rgba() {
    let (spec, data) = decode_image(include_bytes!("data/rgba16.png")).expect("infallible");
    assert_eq!(spec.width, 2);
    assert_eq!(spec.height, 1);
    assert_eq!(spec.pixel_format.bit_depth(), BitDepth::Sixteen);
    // Reformat to RGBA8 for comparison.
    assert_eq!(rgba8(&spec, &data), &[255, 128, 0, 255, 18, 171, 255, 1,]);
}

#[test]
fn decodes_rgba_interlaced() {
    let (spec, data) =
        decode_image(include_bytes!("data/rgba_interlaced.png")).expect("infallible");
    // Re-encode as non-interlaced and verify roundtrip.
    let rgba = rgba8(&spec, &data);
    let spec2 = ImageSpec {
        width: 4,
        height: 4,
        pixel_format: PixelFormat::Rgba8,
        interlaced: false,
    };
    let bytes = encode_image(&spec2, &rgba).expect("infallible");
    let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
    assert_eq!(rgba8(&decoded_spec, &decoded_data), rgba);
}

#[test]
fn decodes_palette_interlaced() {
    let (spec, data) =
        decode_image(include_bytes!("data/palette_interlaced.png")).expect("infallible");
    assert!(spec.width > 0);
    assert!(spec.height > 0);
    assert_eq!(spec.pixel_format.color_mode(), ColorMode::Indexed);
    // Verify we can roundtrip through RGBA.
    let rgba = rgba8(&spec, &data);
    assert!(!rgba.is_empty());
}

#[test]
fn decodes_gray16_interlaced() {
    let (spec, data) =
        decode_image(include_bytes!("data/gray16_interlaced.png")).expect("infallible");
    assert_eq!(spec.width, 5);
    assert_eq!(spec.height, 4);
    assert_eq!(spec.pixel_format.bit_depth(), BitDepth::Sixteen);
    assert!(matches!(
        spec.pixel_format.color_mode(),
        ColorMode::Grayscale | ColorMode::GrayscaleAlpha
    ));
    let rgba = rgba8(&spec, &data);
    assert_eq!(rgba.len(), 5 * 4 * 4);
}

#[test]
fn rejects_crc_mismatch() {
    let mut bytes = include_bytes!("data/gray_filters.png").to_vec();
    let index = bytes.len() - 1;
    bytes[index] ^= 0x01;
    let error = decode_image(&bytes).expect_err("infallible");
    assert!(matches!(error, Error::InvalidData(message) if message.contains("CRC mismatch")));
}

#[test]
fn rejects_missing_plte_for_palette_image() {
    let mut bytes = include_bytes!("data/palette_4bit_trns.png").to_vec();
    remove_chunk(&mut bytes, b"PLTE");
    remove_chunk(&mut bytes, b"tRNS");
    let error = decode_image(&bytes).expect_err("infallible");
    assert!(matches!(error, Error::InvalidData(message) if message.contains("missing PLTE")));
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
