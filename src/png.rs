use alloc::vec::Vec;

use crate::chunk::{IdatChunk, IendChunk, IhdrChunk, PlteChunk, TrnsChunk};
use crate::png_pixels::validate_pixels;

pub use crate::png_pixels::PngPixels;
pub use crate::png_types::{
    Error, PngBitDepth, PngColorMode, PngEncoding, PngImage, PngInfo, Result,
};

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

impl PngInfo {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let header = crate::png_decode::parse_png_header(bytes)?;
        Ok(Self::from_header(&header))
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
            bit_depth: PngBitDepth::from_u8(header.bit_depth)
                .expect("bug: validated bit depth must map to PngBitDepth"),
            color_mode: PngColorMode::from_color_type(header.color_type),
            interlaced: header.interlace_method == 1,
        }
    }
}

impl PngEncoding {
    pub fn for_pixels(pixels: &PngPixels<'_>) -> Self {
        Self {
            color_mode: pixels.color_mode(),
            bit_depth: pixels.bit_depth(),
            interlaced: false,
        }
    }
}

impl<'a> PngImage<'a> {
    pub fn new(
        width: u32,
        height: u32,
        pixels: PngPixels<'a>,
        encoding: PngEncoding,
    ) -> Result<Self> {
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
        validate_pixels(&pixels)?;
        Ok(Self {
            width,
            height,
            pixels,
            encoding,
        })
    }

    /// Decodes PNG bytes into a [`PngPixels`] variant that is closest to the source PNG.
    ///
    /// Low-bit grayscale and indexed images are returned as unpacked samples or
    /// indices. `tRNS` is reflected in the returned pixels, so grayscale or
    /// truecolor images with transparency become `GrayAlpha*` or `Rgba*`.
    ///
    /// This method validates the expected decode sizes implied by `IHDR`, such
    /// as filtered scanline sizes and final decoded output size, and rejects
    /// streams whose decoded layout is inconsistent with those values.
    ///
    /// This method does not impose a caller-configurable size policy. Call
    /// [`PngInfo::from_bytes`] first if you want to reject images based on
    /// width, height, pixel count, or expected decoded RGBA8 size before doing
    /// a full decode.
    pub fn from_bytes(bytes: &[u8]) -> Result<PngImage<'static>> {
        let (header, pixels) = crate::png_decode::decode_png(bytes)?;
        let encoding = PngEncoding::for_pixels(&pixels);
        PngImage::<'static>::new(header.width, header.height, pixels, encoding)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &PngPixels<'a> {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut PngPixels<'a> {
        &mut self.pixels
    }

    pub fn into_pixels(self) -> PngPixels<'a> {
        self.pixels
    }

    pub fn encoding(&self) -> &PngEncoding {
        &self.encoding
    }

    pub fn encoding_mut(&mut self) -> &mut PngEncoding {
        &mut self.encoding
    }

    /// Encodes the image using its current [`PngEncoding`].
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let encoded = crate::png_encode::EncodedImage::from_pixels(
            self.width,
            self.height,
            &self.pixels,
            self.encoding,
        )?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PNG_SIGNATURE);

        IhdrChunk {
            width: self.width,
            height: self.height,
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
        Error, IhdrChunk, PNG_SIGNATURE, PngBitDepth, PngColorMode, PngEncoding, PngImage, PngInfo,
        PngPixels,
    };

    #[test]
    fn roundtrip_rgba_writer_and_reader() {
        let pixels = PngPixels::from_rgba8(vec![255, 0, 0, 255, 0, 255, 0, 128]);
        let image = PngImage::new(2, 1, pixels.clone(), PngEncoding::for_pixels(&pixels))
            .expect("infallible");
        let bytes = image.to_bytes().expect("infallible");
        let decoded = PngImage::from_bytes(&bytes).expect("infallible");
        assert_eq!(
            decoded
                .pixels()
                .to_rgba8()
                .as_u8_slice()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_slice().expect("infallible")
        );
    }

    #[test]
    fn write_to_uses_explicit_indexed_encoding() {
        let pixels = PngPixels::from_rgba8(vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 0, 64,
        ]);
        let mut image = PngImage::new(4, 1, pixels.clone(), PngEncoding::for_pixels(&pixels))
            .expect("infallible");
        *image.encoding_mut() = PngEncoding {
            color_mode: PngColorMode::Indexed,
            bit_depth: PngBitDepth::Two,
            interlaced: false,
        };
        let bytes = image.to_bytes().expect("infallible");
        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 2);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_INDEXED);
        let decoded = PngImage::from_bytes(&bytes).expect("infallible");
        assert_eq!(
            decoded
                .pixels()
                .to_rgba8()
                .as_u8_slice()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_slice().expect("infallible")
        );
    }

    #[test]
    fn borrowed_rgb8_can_be_encoded() {
        let data = [255u8, 0, 0, 0, 255, 0];
        let image = PngImage::new(
            2,
            1,
            PngPixels::from_rgb8(&data[..]),
            PngEncoding {
                color_mode: PngColorMode::Rgb,
                bit_depth: PngBitDepth::Eight,
                interlaced: false,
            },
        )
        .expect("infallible");
        let bytes = image.to_bytes().expect("infallible");
        let decoded = PngImage::from_bytes(&bytes).expect("infallible");
        assert_eq!(
            decoded
                .pixels()
                .to_rgb8()
                .as_u8_slice()
                .expect("infallible"),
            &data
        );
    }

    #[test]
    fn new_rejects_pixel_count_mismatch() {
        let error = PngImage::new(
            2,
            1,
            PngPixels::from_rgba8(vec![0, 1, 2, 3]),
            PngEncoding {
                color_mode: PngColorMode::Rgba,
                bit_depth: PngBitDepth::Eight,
                interlaced: false,
            },
        )
        .unwrap_err();
        assert!(
            matches!(error, Error::InvalidData(message) if message.contains("pixel buffer length"))
        );
    }

    #[test]
    fn new_rejects_index_out_of_range() {
        let error = PngImage::new(
            2,
            1,
            PngPixels::from_indexed2(vec![0, 4], vec![0, 0, 0, 255, 255, 255], None::<Vec<u8>>),
            PngEncoding {
                color_mode: PngColorMode::Indexed,
                bit_depth: PngBitDepth::Two,
                interlaced: false,
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("out-of-range")));
    }

    #[test]
    fn writing_with_sixteen_bit_encoding_writes_sixteen_bit_png() {
        let image = PngImage::new(
            2,
            1,
            PngPixels::from_rgba16(vec![0u16, 1, 2, 3, 65535, 32768, 16, 255]),
            PngEncoding {
                color_mode: PngColorMode::Rgba,
                bit_depth: PngBitDepth::Sixteen,
                interlaced: false,
            },
        )
        .expect("infallible");
        let bytes = image.to_bytes().expect("infallible");
        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 16);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_RGBA);
    }

    #[test]
    fn png_info_rejects_truncated_ihdr() {
        let error = PngInfo::from_bytes(&PNG_SIGNATURE).expect_err("infallible");
        assert!(matches!(error, Error::InvalidData(message) if message.contains("unexpected end")));
    }

    #[test]
    fn new_rejects_zero_width() {
        let error =
            PngImage::new(0, 1, PngPixels::from_rgba8(vec![]), PngEncoding::default()).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("non-zero")));
    }

    #[test]
    fn new_rejects_zero_height() {
        let error =
            PngImage::new(1, 0, PngPixels::from_rgba8(vec![]), PngEncoding::default()).unwrap_err();
        assert!(matches!(error, Error::InvalidData(message) if message.contains("non-zero")));
    }

    #[test]
    fn roundtrip_1x1_rgba() {
        let pixels = PngPixels::from_rgba8(vec![42, 128, 200, 255]);
        let image = PngImage::new(1, 1, pixels.clone(), PngEncoding::for_pixels(&pixels))
            .expect("infallible");
        let bytes = image.to_bytes().expect("infallible");
        let decoded = PngImage::from_bytes(&bytes).expect("infallible");
        assert_eq!(
            decoded
                .pixels()
                .to_rgba8()
                .as_u8_slice()
                .expect("infallible"),
            pixels.to_rgba8().as_u8_slice().expect("infallible")
        );
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
