use nopng::{PngDecodeError, PngRgbaImage};

#[test]
fn decodes_grayscale_png() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/gray_filters.png")).unwrap();
    assert_eq!(image.width(), 3);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            0, 0, 0, 255, 127, 127, 127, 255, 255, 255, 255, 255, 10, 10, 10, 255, 140, 140, 140,
            255, 200, 200, 200, 255,
        ]
    );
}

#[test]
fn decodes_grayscale_alpha_png() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/gray_alpha_avg.png")).unwrap();
    assert_eq!(image.width(), 2);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            20, 20, 20, 255, 180, 180, 180, 64, 100, 100, 100, 128, 220, 220, 220, 200,
        ]
    );
}

#[test]
fn decodes_rgb_png() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/rgb_sub_up.png")).unwrap();
    assert_eq!(image.width(), 2);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ]
    );
}

#[test]
fn decodes_rgba_png_with_split_idat() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/rgba_paeth_split_idat.png")).unwrap();
    assert_eq!(image.width(), 2);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 64,
        ]
    );
}

#[test]
fn decodes_1bit_grayscale_png() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/gray_1bit_filters.png")).unwrap();
    assert_eq!(image.width(), 5);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ]
    );
}

#[test]
fn decodes_2bit_grayscale_with_trns() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/gray_2bit_trns.png")).unwrap();
    assert_eq!(image.width(), 4);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            0, 0, 0, 255, 85, 85, 85, 0, 170, 170, 170, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 170, 170, 170, 255, 85, 85, 85, 0, 0, 0, 0, 255,
        ]
    );
}

#[test]
fn decodes_4bit_palette_with_trns() {
    let image = PngRgbaImage::from_bytes(include_bytes!("data/palette_4bit_trns.png")).unwrap();
    assert_eq!(image.width(), 4);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 0, 255, 255, 255, 0, 255, 0, 0,
            255, 0, 0, 255, 0, 128, 255, 0, 0, 255,
        ]
    );
}

#[test]
fn decodes_8bit_palette_with_split_idat() {
    let image =
        PngRgbaImage::from_bytes(include_bytes!("data/palette_8bit_split_idat.png")).unwrap();
    assert_eq!(image.width(), 3);
    assert_eq!(image.height(), 2);
    assert_eq!(
        image.data(),
        &[
            0, 0, 0, 255, 120, 10, 200, 255, 255, 255, 255, 255, 255, 255, 255, 255, 120, 10, 200,
            255, 0, 0, 0, 255,
        ]
    );
}

#[test]
fn rejects_crc_mismatch() {
    let mut bytes = include_bytes!("data/gray_filters.png").to_vec();
    let index = bytes.len() - 1;
    bytes[index] ^= 0x01;
    let error = PngRgbaImage::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(error, PngDecodeError::InvalidChunk(message) if message.contains("CRC mismatch"))
    );
}

#[test]
fn rejects_missing_plte_for_palette_image() {
    let mut bytes = include_bytes!("data/palette_4bit_trns.png").to_vec();
    remove_chunk(&mut bytes, b"PLTE");
    remove_chunk(&mut bytes, b"tRNS");
    let error = PngRgbaImage::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(error, PngDecodeError::InvalidChunk(message) if message.contains("missing PLTE"))
    );
}

#[test]
fn rejects_plte_after_idat() {
    let bytes = reorder_chunks_after_idat(
        include_bytes!("data/palette_4bit_trns.png"),
        &[*b"PLTE", *b"tRNS"],
    );
    let error = PngRgbaImage::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(error, PngDecodeError::InvalidChunk(message) if message.contains("PLTE appears after IDAT"))
    );
}

#[test]
fn rejects_trns_longer_than_palette() {
    let bytes = replace_chunk(
        include_bytes!("data/palette_4bit_trns.png"),
        b"tRNS",
        &[255, 128, 0, 255, 12],
    );
    let error = PngRgbaImage::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(error, PngDecodeError::InvalidChunk(message) if message.contains("tRNS length exceeds palette length"))
    );
}

#[test]
fn rejects_palette_index_out_of_range() {
    let bytes = replace_chunk(
        include_bytes!("data/palette_8bit_split_idat.png"),
        b"PLTE",
        &[0, 0, 0],
    );
    let error = PngRgbaImage::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(error, PngDecodeError::InvalidData(message) if message.contains("palette index out of range"))
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

fn reorder_chunks_after_idat(bytes: &[u8], chunk_types: &[[u8; 4]]) -> Vec<u8> {
    let mut chunks = collect_chunks(bytes);
    let mut moved = Vec::new();
    chunks.retain(|chunk| {
        if chunk_types.contains(&chunk.chunk_type) {
            moved.push(chunk.clone());
            false
        } else {
            true
        }
    });
    let insert_at = chunks
        .iter()
        .position(|chunk| chunk.chunk_type == *b"IEND")
        .unwrap();
    for (offset, chunk) in moved.into_iter().enumerate() {
        chunks.insert(insert_at + offset, chunk);
    }
    rebuild_png(&chunks)
}

fn replace_chunk(bytes: &[u8], chunk_type: &[u8; 4], new_data: &[u8]) -> Vec<u8> {
    let mut chunks = collect_chunks(bytes);
    let chunk = chunks
        .iter_mut()
        .find(|chunk| &chunk.chunk_type == chunk_type)
        .unwrap();
    chunk.data = new_data.to_vec();
    rebuild_png(&chunks)
}

#[derive(Clone)]
struct Chunk {
    chunk_type: [u8; 4],
    data: Vec<u8>,
}

fn collect_chunks(bytes: &[u8]) -> Vec<Chunk> {
    let mut offset = 8;
    let mut chunks = Vec::new();
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let chunk_type: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
        offset += 4;
        let data = bytes[offset..offset + length].to_vec();
        offset += length + 4;
        chunks.push(Chunk { chunk_type, data });
    }
    chunks
}

fn rebuild_png(chunks: &[Chunk]) -> Vec<u8> {
    let mut bytes = Vec::from(b"\x89PNG\r\n\x1a\n".as_slice());
    for chunk in chunks {
        bytes.extend_from_slice(&(chunk.data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&chunk.chunk_type);
        bytes.extend_from_slice(&chunk.data);
        let crc = crc32(&chunk.chunk_type, &chunk.data);
        bytes.extend_from_slice(&crc.to_be_bytes());
    }
    bytes
}

fn crc32(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in chunk_type.iter().chain(data.iter()) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = 0xEDB8_8320 ^ (crc >> 1);
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFF_FFFF
}
