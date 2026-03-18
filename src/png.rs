use alloc::vec::Vec;

use crate::chunk::{IdatChunk, IendChunk, IhdrChunk, PlteChunk, TrnsChunk};
use crate::png_pixels::validate_pixels;

pub use crate::png_pixels::Pixels;
pub use crate::png_types::{BitDepth, ColorMode, Error, Result};

pub(crate) const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
pub(crate) const ADAM7_PASSES: [Adam7Pass; 7] = [
    Adam7Pass {
        x_start: 0,
        y_start: 0,
        x_step: 8,
        y_step: 8,
    },
    Adam7Pass {
        x_start: 4,
        y_start: 0,
        x_step: 8,
        y_step: 8,
    },
    Adam7Pass {
        x_start: 0,
        y_start: 4,
        x_step: 4,
        y_step: 8,
    },
    Adam7Pass {
        x_start: 2,
        y_start: 0,
        x_step: 4,
        y_step: 4,
    },
    Adam7Pass {
        x_start: 0,
        y_start: 2,
        x_step: 2,
        y_step: 4,
    },
    Adam7Pass {
        x_start: 1,
        y_start: 0,
        x_step: 2,
        y_step: 2,
    },
    Adam7Pass {
        x_start: 0,
        y_start: 1,
        x_step: 1,
        y_step: 2,
    },
];

/// Image metadata describing dimensions, bit depth, color mode and interlacing.
///
/// Use [`ImageSpec::from_bytes`] for a cheap preflight check that only reads
/// the IHDR chunk, or [`ImageSpec::from_pixels`] to derive a spec from pixel
/// data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSpec {
    pub width: u32,
    pub height: u32,
    pub bit_depth: BitDepth,
    pub color_mode: ColorMode,
    pub interlaced: bool,
}

impl ImageSpec {
    /// Reads basic PNG information from the PNG signature and `IHDR` chunk.
    ///
    /// This is intended for cheap preflight checks such as rejecting images
    /// whose dimensions are too large. It does not perform full PNG validation
    /// and does not inspect later chunks such as `PLTE`, `tRNS` or `IDAT`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let header = crate::png_decode::parse_png_header(bytes)?;
        Ok(Self::from_header(&header))
    }

    /// Derives an `ImageSpec` from pixel data and dimensions.
    ///
    /// The `bit_depth` and `color_mode` are inferred from the pixel variant.
    /// Interlacing defaults to `false`.
    pub fn from_pixels(width: u32, height: u32, pixels: &Pixels<'_>) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidData(
                "image dimensions must be non-zero".into(),
            ));
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
        if pixels.pixel_count() != expected {
            return Err(Error::InvalidData(
                "image size does not match pixel buffer length".into(),
            ));
        }
        Ok(Self {
            width,
            height,
            color_mode: pixels.color_mode(),
            bit_depth: pixels.bit_depth(),
            interlaced: false,
        })
    }

    pub fn pixel_count(&self) -> Option<usize> {
        (self.width as usize).checked_mul(self.height as usize)
    }

    pub fn decoded_rgba8_bytes(&self) -> Option<usize> {
        self.pixel_count()?.checked_mul(4)
    }

    pub fn filtered_bytes(&self) -> Option<usize> {
        let header = crate::png_decode::PngHeader::new(
            self.width,
            self.height,
            self.bit_depth.as_u8(),
            self.color_mode.to_color_type(),
            u8::from(self.interlaced),
        );
        crate::png_decode::expected_filtered_len(&header).ok()
    }

    fn from_header(header: &crate::png_decode::PngHeader) -> Self {
        Self {
            width: header.width,
            height: header.height,
            bit_depth: BitDepth::from_u8(header.bit_depth)
                .expect("bug: validated bit depth must map to BitDepth"),
            color_mode: ColorMode::from_color_type(header.color_type),
            interlaced: header.interlace_method == 1,
        }
    }
}

/// Decodes PNG bytes into an [`ImageSpec`] and [`Pixels`] variant closest to
/// the source PNG.
///
/// Low-bit grayscale and indexed images are returned as unpacked samples or
/// indices. `tRNS` is reflected in the returned pixels, so grayscale or
/// truecolor images with transparency become `GrayAlpha*` or `Rgba*`.
///
/// This function validates the expected decode sizes implied by `IHDR`, such
/// as filtered scanline sizes and final decoded output size, and rejects
/// streams whose decoded layout is inconsistent with those values.
///
/// This function does not impose a caller-configurable size policy. Call
/// [`ImageSpec::from_bytes`] first if you want to reject images based on
/// width, height, pixel count, or expected decoded RGBA8 size before doing
/// a full decode.
pub fn decode_image(bytes: &[u8]) -> Result<(ImageSpec, Pixels<'static>)> {
    let (header, pixels) = crate::png_decode::decode_png(bytes)?;
    let spec = ImageSpec {
        width: header.width,
        height: header.height,
        bit_depth: pixels.bit_depth(),
        color_mode: pixels.color_mode(),
        interlaced: header.interlace_method == 1,
    };
    validate_spec_and_pixels(&spec, &pixels)?;
    Ok((spec, pixels))
}

/// Encodes an image described by `spec` and `pixels` into PNG bytes.
///
/// The `spec` determines the output format (color mode, bit depth, interlacing).
/// Dimensions in `spec` must match the pixel count in `pixels`.
pub fn encode_image(spec: &ImageSpec, pixels: &Pixels<'_>) -> Result<Vec<u8>> {
    validate_spec_and_pixels(spec, pixels)?;
    validate_pixels(pixels)?;
    validate_encoding_compatibility(pixels, spec)?;

    let encoded =
        crate::png_encode::EncodedImage::from_pixels(spec.width, spec.height, pixels, spec)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&PNG_SIGNATURE);

    IhdrChunk {
        width: spec.width,
        height: spec.height,
        bit_depth: encoded.bit_depth,
        color_type: encoded.color_type,
        interlace_method: encoded.interlace_method,
    }
    .append_to(&mut bytes);
    if let Some(palette) = encoded.palette.as_deref() {
        PlteChunk { palette }.append_to(&mut bytes);
    }
    if let Some(trns) = encoded.trns.as_deref() {
        TrnsChunk { data: trns }.append_to(&mut bytes);
    }
    IdatChunk {
        filtered_data: &encoded.filtered_data,
    }
    .append_to(&mut bytes)?;
    IendChunk.append_to(&mut bytes);

    Ok(bytes)
}

fn validate_spec_and_pixels(spec: &ImageSpec, pixels: &Pixels<'_>) -> Result<()> {
    if spec.width == 0 || spec.height == 0 {
        return Err(Error::InvalidData(
            "image dimensions must be non-zero".into(),
        ));
    }
    let expected = (spec.width as usize)
        .checked_mul(spec.height as usize)
        .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
    if pixels.pixel_count() != expected {
        return Err(Error::InvalidData(
            "image size does not match pixel buffer length".into(),
        ));
    }
    Ok(())
}

fn validate_encoding_compatibility(pixels: &Pixels<'_>, spec: &ImageSpec) -> Result<()> {
    let is_16bit_pixels = matches!(pixels.bit_depth(), BitDepth::Sixteen);
    let is_16bit_encoding = spec.bit_depth == BitDepth::Sixteen;

    if is_16bit_pixels != is_16bit_encoding {
        return Err(Error::Unsupported(
            "16-bit pixels require 16-bit encoding and vice versa".into(),
        ));
    }

    if spec.color_mode == ColorMode::Indexed && is_16bit_encoding {
        return Err(Error::Unsupported(
            "16-bit indexed encoding is not supported".into(),
        ));
    }

    Ok(())
}

pub(crate) fn adam7_axis_size(size: u32, start: u8, step: u8) -> u32 {
    if size <= u32::from(start) {
        0
    } else {
        (size - u32::from(start)).div_ceil(u32::from(step))
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Adam7Pass {
    pub(crate) x_start: u8,
    pub(crate) y_start: u8,
    pub(crate) x_step: u8,
    pub(crate) y_step: u8,
}

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};

    use super::{
        BitDepth, ColorMode, Error, IhdrChunk, ImageSpec, PNG_SIGNATURE, Pixels, decode_image,
        encode_image,
    };

    #[test]
    fn roundtrip_rgba_writer_and_reader() {
        let pixels = Pixels::Rgba8(vec![255, 0, 0, 255, 0, 255, 0, 128].into());
        let spec = ImageSpec::from_pixels(2, 1, &pixels).expect("infallible");
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgba8()
                .as_u8_storage()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_storage().expect("infallible")
        );
    }

    #[test]
    fn write_to_uses_explicit_indexed_encoding() {
        let pixels = Pixels::Rgba8(
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 0, 64,
            ]
            .into(),
        );
        let spec = ImageSpec {
            width: 4,
            height: 1,
            color_mode: ColorMode::Indexed,
            bit_depth: BitDepth::Two,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 2);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_INDEXED);
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgba8()
                .as_u8_storage()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_storage().expect("infallible")
        );
    }

    #[test]
    fn borrowed_rgb8_can_be_encoded() {
        let data = [255u8, 0, 0, 0, 255, 0];
        let pixels = Pixels::Rgb8((&data[..]).into());
        let spec = ImageSpec {
            width: 2,
            height: 1,
            color_mode: ColorMode::Rgb,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgb8()
                .as_u8_storage()
                .expect("infallible"),
            &data
        );
    }

    #[test]
    fn new_rejects_pixel_count_mismatch() {
        let pixels = Pixels::Rgba8(vec![0, 1, 2, 3].into());
        let spec = ImageSpec {
            width: 2,
            height: 1,
            color_mode: ColorMode::Rgba,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let error = encode_image(&spec, &pixels).unwrap_err();
        assert!(
            matches!(error, Error::InvalidData(message) if message.contains("pixel buffer length"))
        );
    }

    #[test]
    fn new_rejects_index_out_of_range() {
        let pixels = Pixels::Indexed {
            bit_depth: BitDepth::Two,
            indices: vec![0, 4].into(),
            palette: vec![0, 0, 0, 255, 255, 255].into(),
            trns: None,
        };
        let spec = ImageSpec {
            width: 2,
            height: 1,
            color_mode: ColorMode::Indexed,
            bit_depth: BitDepth::Two,
            interlaced: false,
        };
        let error = encode_image(&spec, &pixels).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("out-of-range")));
    }

    #[test]
    fn writing_with_sixteen_bit_encoding_writes_sixteen_bit_png() {
        let pixels = Pixels::Rgba16(vec![0u16, 1, 2, 3, 65535, 32768, 16, 255].into());
        let spec = ImageSpec {
            width: 2,
            height: 1,
            color_mode: ColorMode::Rgba,
            bit_depth: BitDepth::Sixteen,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 16);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_RGBA);
    }

    #[test]
    fn image_spec_rejects_truncated_ihdr() {
        let error = ImageSpec::from_bytes(&PNG_SIGNATURE).expect_err("infallible");
        assert!(matches!(error, Error::InvalidData(message) if message.contains("unexpected end")));
    }

    #[test]
    fn new_rejects_zero_width() {
        let pixels = Pixels::Rgba8(vec![].into());
        let spec = ImageSpec {
            width: 0,
            height: 1,
            color_mode: ColorMode::Rgba,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let error = encode_image(&spec, &pixels).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("non-zero")));
    }

    #[test]
    fn new_rejects_zero_height() {
        let pixels = Pixels::Rgba8(vec![].into());
        let spec = ImageSpec {
            width: 1,
            height: 0,
            color_mode: ColorMode::Rgba,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let error = encode_image(&spec, &pixels).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("non-zero")));
    }

    #[test]
    fn roundtrip_1x1_rgba() {
        let pixels = Pixels::Rgba8(vec![42, 128, 200, 255].into());
        let spec = ImageSpec::from_pixels(1, 1, &pixels).expect("infallible");
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgba8()
                .as_u8_storage()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_storage().expect("infallible")
        );
    }

    #[test]
    fn roundtrip_grayscale_alpha() {
        let pixels = Pixels::GrayAlpha8(vec![0, 255, 128, 64, 255, 128, 50, 200, 200, 100].into());
        let spec = ImageSpec {
            width: 5,
            height: 1,
            color_mode: ColorMode::GrayscaleAlpha,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgba8()
                .as_u8_storage()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_storage().expect("infallible")
        );
    }

    #[test]
    fn roundtrip_indexed_with_alpha() {
        let pixels = Pixels::Rgba8(
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 64,
            ]
            .into(),
        );
        let spec = ImageSpec {
            width: 2,
            height: 2,
            color_mode: ColorMode::Indexed,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgba8()
                .as_u8_storage()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_storage().expect("infallible")
        );
    }

    #[test]
    fn roundtrip_16bit_grayscale() {
        let pixels = Pixels::Gray16(vec![0, 32768, 65535, 1000].into());
        let spec = ImageSpec {
            width: 2,
            height: 2,
            color_mode: ColorMode::Grayscale,
            bit_depth: BitDepth::Sixteen,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels.as_u16_storage().expect("infallible"),
            pixels.as_u16_storage().expect("infallible")
        );
    }

    #[test]
    fn roundtrip_indexed_direct_fast_path() {
        let indices = vec![0, 1, 2, 0, 1, 2];
        let palette = vec![255, 0, 0, 0, 255, 0, 0, 0, 255];
        let trns: Vec<u8> = vec![255, 128, 0];
        let pixels = Pixels::Indexed {
            bit_depth: BitDepth::Eight,
            indices: indices.clone().into(),
            palette: palette.clone().into(),
            trns: Some(trns.clone().into()),
        };
        let spec = ImageSpec {
            width: 3,
            height: 2,
            color_mode: ColorMode::Indexed,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &pixels).expect("infallible");
        let (_, decoded_pixels) = decode_image(&bytes).expect("infallible");
        assert_eq!(
            decoded_pixels
                .to_rgba8()
                .as_u8_storage()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_storage().expect("infallible")
        );
    }

    #[test]
    fn rejects_16bit_pixels_with_8bit_encoding() {
        let pixels = Pixels::Rgba16(vec![0, 0, 0, 0].into());
        let spec = ImageSpec {
            width: 1,
            height: 1,
            color_mode: ColorMode::Rgba,
            bit_depth: BitDepth::Eight,
            interlaced: false,
        };
        let error = encode_image(&spec, &pixels).unwrap_err();
        assert!(matches!(error, Error::Unsupported(_)));
    }

    struct IhdrInfo {
        bit_depth: u8,
        color_type: u8,
    }

    fn read_ihdr(bytes: &[u8]) -> IhdrInfo {
        let ihdr = find_chunk(bytes, b"IHDR").expect("infallible");
        IhdrInfo {
            bit_depth: ihdr[8],
            color_type: ihdr[9],
        }
    }

    fn find_chunk<'a>(bytes: &'a [u8], chunk_type: &[u8; 4]) -> Option<&'a [u8]> {
        let mut offset = 8;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("bug: chunk length must be 4 bytes"),
            ) as usize;
            offset += 4;
            let current_type: [u8; 4] = bytes[offset..offset + 4]
                .try_into()
                .expect("bug: chunk type must be 4 bytes");
            offset += 4;
            let data = &bytes[offset..offset + length];
            offset += length + 4;
            if &current_type == chunk_type {
                return Some(data);
            }
        }
        None
    }
}
