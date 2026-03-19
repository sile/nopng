use alloc::vec::Vec;

use crate::chunk::{IdatChunk, IendChunk, IhdrChunk, PlteChunk, TrnsChunk};
use crate::pixel_reformat::{reformat, validate_format_and_data};

use crate::png_types::Result;
pub use crate::png_types::{Error, PixelFormat};

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

/// Image metadata describing dimensions, pixel format and interlacing.
///
/// `ImageSpec` serves two roles:
///
/// - **Decode output** — returned by [`decode_image`] and [`inspect_image`]
///   to describe the decoded (or to-be-decoded) pixel data.
/// - **Encode input** — passed to [`encode_image`] to specify how pixel data
///   should be written into a PNG stream.
///
/// # Constraints
///
/// `width` and `height` must both be non-zero. [`encode_image`] and
/// [`decode_image`] return [`Error::InvalidData`] when given a zero dimension.
/// The `pixel_format` (and its embedded palette/trns for indexed variants) is
/// likewise validated at encode/decode time.
///
/// # Pixel data layout
///
/// The accompanying pixel buffer is a flat `&[u8]` laid out in row-major order
/// (top-to-bottom, left-to-right) according to [`pixel_format`](Self::pixel_format).
/// Its expected byte length is given by [`data_len`](Self::data_len).
///
/// # Examples
///
/// Decoding:
///
/// ```
/// # let png_bytes = nopng::encode_image(
/// #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
/// #     &[255, 0, 0, 255],
/// # )?;
/// let (spec, pixels) = nopng::decode_image(&png_bytes)?;
/// assert_eq!(pixels.len(), spec.data_len());
/// # Ok::<(), nopng::Error>(())
/// ```
///
/// Encoding:
///
/// ```
/// let spec = nopng::ImageSpec::new(2, 2, nopng::PixelFormat::Rgba8);
/// let pixels = vec![0u8; spec.data_len()];
/// let png_bytes = nopng::encode_image(&spec, &pixels)?;
/// # Ok::<(), nopng::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSpec {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Pixel format describing how samples are stored in the data buffer.
    pub pixel_format: PixelFormat,
    /// `true` if the image uses Adam7 interlacing.
    pub interlaced: bool,
}

impl ImageSpec {
    /// Creates a new non-interlaced image spec.
    pub const fn new(width: u32, height: u32, pixel_format: PixelFormat) -> Self {
        Self {
            width,
            height,
            pixel_format,
            interlaced: false,
        }
    }

    /// Expected byte length of the pixel data buffer for this spec.
    ///
    /// # Panics
    ///
    /// Panics if the total size overflows `usize`.
    pub fn data_len(&self) -> usize {
        self.pixel_format.data_len(self.width, self.height)
    }

    fn from_header_and_ancillary(
        header: &crate::png_decode::PngHeader,
        ancillary: &crate::png_decode::AncillaryChunks,
    ) -> Self {
        let pixel_format = pixel_format_from_header(header, ancillary);
        Self {
            width: header.width,
            height: header.height,
            pixel_format,
            interlaced: header.interlace_method == 1,
        }
    }
}

/// Reads PNG metadata from the PNG signature, `IHDR`, `PLTE`, and `tRNS`
/// chunks, stopping at the first `IDAT`.
///
/// The returned `pixel_format` reflects the decode output format:
/// - Indexed images include the embedded palette and tRNS.
/// - Grayscale/RGB with tRNS are promoted to alpha variants.
pub fn inspect_image(bytes: &[u8]) -> Result<ImageSpec> {
    let (header, ancillary) = crate::png_decode::parse_png_metadata(bytes)?;
    Ok(ImageSpec::from_header_and_ancillary(&header, &ancillary))
}

/// Determines the decode output `PixelFormat` from header + ancillary chunks.
fn pixel_format_from_header(
    header: &crate::png_decode::PngHeader,
    ancillary: &crate::png_decode::AncillaryChunks,
) -> PixelFormat {
    match (header.color_type, header.bit_depth) {
        (0, bd @ (1 | 2 | 4)) => {
            if ancillary.has_transparency() {
                PixelFormat::GrayAlpha8
            } else {
                match bd {
                    1 => PixelFormat::Gray1,
                    2 => PixelFormat::Gray2,
                    4 => PixelFormat::Gray4,
                    _ => unreachable!(),
                }
            }
        }
        (0, 8) => {
            if ancillary.has_transparency() {
                PixelFormat::GrayAlpha8
            } else {
                PixelFormat::Gray8
            }
        }
        (0, 16) => {
            if ancillary.has_transparency() {
                PixelFormat::GrayAlpha16Be
            } else {
                PixelFormat::Gray16Be
            }
        }
        (2, 8) => {
            if ancillary.has_transparency() {
                PixelFormat::Rgba8
            } else {
                PixelFormat::Rgb8
            }
        }
        (2, 16) => {
            if ancillary.has_transparency() {
                PixelFormat::Rgba16Be
            } else {
                PixelFormat::Rgb16Be
            }
        }
        (3, bd) => {
            let palette = ancillary.flat_palette().unwrap_or_default();
            let trns = ancillary.indexed_trns();
            match bd {
                1 => PixelFormat::Indexed1 { palette, trns },
                2 => PixelFormat::Indexed2 { palette, trns },
                4 => PixelFormat::Indexed4 { palette, trns },
                8 => PixelFormat::Indexed8 { palette, trns },
                _ => unreachable!(),
            }
        }
        (4, 8) => PixelFormat::GrayAlpha8,
        (4, 16) => PixelFormat::GrayAlpha16Be,
        (6, 8) => PixelFormat::Rgba8,
        (6, 16) => PixelFormat::Rgba16Be,
        _ => unreachable!(),
    }
}

/// Decodes PNG bytes into an [`ImageSpec`] and pixel data.
///
/// The pixel data is returned in the PNG's native format. To convert to a
/// different format, use [`reformat_pixels`] on the result.
///
/// Low-bit grayscale and indexed images are returned as unpacked samples or
/// indices (one byte per sample/index). 16-bit samples are in big-endian byte
/// order. `tRNS` transparency is reflected: grayscale or truecolor images with
/// `tRNS` become alpha variants.
///
/// # Examples
///
/// ```
/// # let png_bytes = nopng::encode_image(
/// #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Gray8),
/// #     &[128],
/// # )?;
/// // Decode in native format
/// let (spec, pixels) = nopng::decode_image(&png_bytes)?;
///
/// // Convert to RGBA8 if needed
/// let rgba = nopng::reformat_pixels(&spec.pixel_format, &pixels, &nopng::PixelFormat::Rgba8)?;
/// # Ok::<(), nopng::Error>(())
/// ```
pub fn decode_image(bytes: &[u8]) -> Result<(ImageSpec, Vec<u8>)> {
    let (header, _ancillary, native_format, data) = crate::png_decode::decode_png(bytes)?;
    let spec = ImageSpec {
        width: header.width,
        height: header.height,
        pixel_format: native_format,
        interlaced: header.interlace_method == 1,
    };
    validate_format_and_data(&spec.pixel_format, &data, spec.width, spec.height)?;
    Ok((spec, data))
}

/// Converts pixel data from one [`PixelFormat`] to another.
///
/// This function works on any pixel data, not just data from [`decode_image`].
/// It can be used to convert between formats before encoding or after decoding.
///
/// # Limitations
///
/// Converting **to** an indexed format (`Indexed1`/`Indexed2`/`Indexed4`/`Indexed8`)
/// is not supported and returns [`Error::Unsupported`].
///
/// # Examples
///
/// ```
/// # let png_bytes = nopng::encode_image(
/// #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Gray8),
/// #     &[128],
/// # )?;
/// let (spec, pixels) = nopng::decode_image(&png_bytes)?;
/// let rgba = nopng::reformat_pixels(&spec.pixel_format, &pixels, &nopng::PixelFormat::Rgba8)?;
/// # Ok::<(), nopng::Error>(())
/// ```
pub fn reformat_pixels(
    src_fmt: &PixelFormat,
    src: &[u8],
    dst_fmt: &PixelFormat,
) -> Result<Vec<u8>> {
    reformat(src_fmt, src, dst_fmt)
}

/// Encodes an image described by `spec` into PNG bytes.
///
/// The `data` buffer must contain pixel data in the format described by
/// `spec.pixel_format`, with length matching [`ImageSpec::data_len()`].
pub fn encode_image(spec: &ImageSpec, data: &[u8]) -> Result<Vec<u8>> {
    validate_format_and_data(&spec.pixel_format, data, spec.width, spec.height)?;

    let encoded = crate::png_encode::EncodedImage::from_format_and_data(
        spec.width,
        spec.height,
        &spec.pixel_format,
        data,
        spec.interlaced,
    )?;
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
        Error, IhdrChunk, ImageSpec, PNG_SIGNATURE, PixelFormat, decode_image, encode_image,
        inspect_image,
    };
    use crate::pixel_reformat::reformat;

    #[test]
    fn roundtrip_rgba_writer_and_reader() {
        let data = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let spec = ImageSpec {
            width: 2,
            height: 1,
            pixel_format: PixelFormat::Rgba8,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &data).expect("infallible");
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgba = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");
        let expected_rgba =
            reformat(&spec.pixel_format, &data, &PixelFormat::Rgba8).expect("infallible");
        assert_eq!(decoded_rgba, expected_rgba);
    }

    #[test]
    fn write_to_uses_explicit_indexed_encoding() {
        let indices = vec![0, 1, 2, 3];
        let palette = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0];
        let trns = vec![255, 128, 255, 64];
        let spec = ImageSpec {
            width: 4,
            height: 1,
            pixel_format: PixelFormat::Indexed2 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            interlaced: false,
        };
        let bytes = encode_image(&spec, &indices).expect("infallible");
        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 2);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_INDEXED);
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgba = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");
        let original_rgba =
            reformat(&spec.pixel_format, &indices, &PixelFormat::Rgba8).expect("infallible");
        assert_eq!(decoded_rgba, original_rgba);
    }

    #[test]
    fn borrowed_rgb8_can_be_encoded() {
        let data = [255u8, 0, 0, 0, 255, 0];
        let spec = ImageSpec {
            width: 2,
            height: 1,
            pixel_format: PixelFormat::Rgb8,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &data).expect("infallible");
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgb = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgb8,
        )
        .expect("infallible");
        assert_eq!(decoded_rgb, &data);
    }

    #[test]
    fn new_rejects_pixel_count_mismatch() {
        let data = vec![0, 1, 2, 3];
        let spec = ImageSpec {
            width: 2,
            height: 1,
            pixel_format: PixelFormat::Rgba8,
            interlaced: false,
        };
        let error = encode_image(&spec, &data).unwrap_err();
        assert!(
            matches!(error, Error::InvalidData(message) if message.contains("pixel buffer length"))
        );
    }

    #[test]
    fn new_rejects_index_out_of_range() {
        let data = vec![0, 4];
        let spec = ImageSpec {
            width: 2,
            height: 1,
            pixel_format: PixelFormat::Indexed2 {
                palette: vec![0, 0, 0, 255, 255, 255],
                trns: None,
            },
            interlaced: false,
        };
        let error = encode_image(&spec, &data).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("out-of-range")));
    }

    #[test]
    fn writing_with_sixteen_bit_encoding_writes_sixteen_bit_png() {
        // 2 pixels × 8 bytes per pixel (RGBA16Be)
        let data: Vec<u8> = [0u16, 1, 2, 3, 65535, 32768, 16, 255]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let spec = ImageSpec {
            width: 2,
            height: 1,
            pixel_format: PixelFormat::Rgba16Be,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &data).expect("infallible");
        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 16);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_RGBA);
    }

    #[test]
    fn image_spec_rejects_truncated_ihdr() {
        let error = inspect_image(&PNG_SIGNATURE).expect_err("infallible");
        assert!(matches!(error, Error::InvalidData(message) if message.contains("IHDR")));
    }

    #[test]
    fn new_rejects_zero_width() {
        let data = vec![];
        let spec = ImageSpec {
            width: 0,
            height: 1,
            pixel_format: PixelFormat::Rgba8,
            interlaced: false,
        };
        let error = encode_image(&spec, &data).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("non-zero")));
    }

    #[test]
    fn new_rejects_zero_height() {
        let data = vec![];
        let spec = ImageSpec {
            width: 1,
            height: 0,
            pixel_format: PixelFormat::Rgba8,
            interlaced: false,
        };
        let error = encode_image(&spec, &data).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("non-zero")));
    }

    #[test]
    fn roundtrip_1x1_rgba() {
        let data = vec![42, 128, 200, 255];
        let spec = ImageSpec {
            width: 1,
            height: 1,
            pixel_format: PixelFormat::Rgba8,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &data).expect("infallible");
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgba = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");
        assert_eq!(decoded_rgba, data);
    }

    #[test]
    fn roundtrip_grayscale_alpha() {
        let data = vec![0, 255, 128, 64, 255, 128, 50, 200, 200, 100];
        let spec = ImageSpec {
            width: 5,
            height: 1,
            pixel_format: PixelFormat::GrayAlpha8,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &data).expect("infallible");
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgba = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");
        let original_rgba =
            reformat(&spec.pixel_format, &data, &PixelFormat::Rgba8).expect("infallible");
        assert_eq!(decoded_rgba, original_rgba);
    }

    #[test]
    fn roundtrip_indexed_with_alpha() {
        let indices = vec![0, 1, 2, 3];
        let palette = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let trns = vec![255, 128, 0, 64];
        let spec = ImageSpec {
            width: 2,
            height: 2,
            pixel_format: PixelFormat::Indexed8 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            interlaced: false,
        };
        let bytes = encode_image(&spec, &indices).expect("infallible");
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgba = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");
        let original_rgba =
            reformat(&spec.pixel_format, &indices, &PixelFormat::Rgba8).expect("infallible");
        assert_eq!(decoded_rgba, original_rgba);
    }

    #[test]
    fn roundtrip_16bit_grayscale() {
        // 4 pixels × 2 bytes per pixel (Gray16Be)
        let data: Vec<u8> = [0u16, 32768, 65535, 1000]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        let spec = ImageSpec {
            width: 2,
            height: 2,
            pixel_format: PixelFormat::Gray16Be,
            interlaced: false,
        };
        let bytes = encode_image(&spec, &data).expect("infallible");
        let (_, decoded_data) = decode_image(&bytes).expect("infallible");
        assert_eq!(decoded_data, data);
    }

    #[test]
    fn roundtrip_indexed_direct_fast_path() {
        let indices = vec![0, 1, 2, 0, 1, 2];
        let palette = vec![255, 0, 0, 0, 255, 0, 0, 0, 255];
        let trns: Vec<u8> = vec![255, 128, 0];
        let spec = ImageSpec {
            width: 3,
            height: 2,
            pixel_format: PixelFormat::Indexed8 {
                palette: palette.clone(),
                trns: Some(trns.clone()),
            },
            interlaced: false,
        };
        let bytes = encode_image(&spec, &indices).expect("infallible");
        let (decoded_spec, decoded_data) = decode_image(&bytes).expect("infallible");
        let decoded_rgba = reformat(
            &decoded_spec.pixel_format,
            &decoded_data,
            &PixelFormat::Rgba8,
        )
        .expect("infallible");
        let original_rgba =
            reformat(&spec.pixel_format, &indices, &PixelFormat::Rgba8).expect("infallible");
        assert_eq!(decoded_rgba, original_rgba);
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
