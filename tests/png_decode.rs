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
fn rejects_crc_mismatch() {
    let mut bytes = include_bytes!("data/gray_filters.png").to_vec();
    let index = bytes.len() - 1;
    bytes[index] ^= 0x01;
    let error = PngRgbaImage::from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(error, PngDecodeError::InvalidChunk(message) if message.contains("CRC mismatch"))
    );
}
