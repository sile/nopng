use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::chunk::{IdatChunk, IendChunk, IhdrChunk, PlteChunk, TrnsChunk};
use crate::png_pixels::{
    downsample_u16, flatten_palette, scale_sample_to_u8, upscale_u8_to_u16, validate_pixels,
};
use crate::{adler32, crc, deflate};

pub use crate::png_pixels::PngPixels;
pub use crate::png_types::{
    Error, PngBitDepth, PngColorMode, PngEncoding, PngImage, PngInfo, Result,
};

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const ADAM7_PASSES: [Adam7Pass; 7] = [
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
        let header = parse_png_header(bytes)?;
        Ok(Self::from_header(&header))
    }

    pub fn pixel_count(&self) -> Option<usize> {
        (self.width as usize).checked_mul(self.height as usize)
    }

    pub fn decoded_rgba8_bytes(&self) -> Option<usize> {
        self.pixel_count()?.checked_mul(4)
    }

    pub fn filtered_bytes(&self) -> Option<usize> {
        let header = PngHeader {
            width: self.width,
            height: self.height,
            bit_depth: self.bit_depth.as_u8(),
            color_type: color_type_from_color_mode(self.color_mode),
            compression_method: 0,
            filter_method: 0,
            interlace_method: u8::from(self.interlaced),
        };
        expected_filtered_len(&header).ok()
    }

    fn from_header(header: &PngHeader) -> Self {
        Self {
            width: header.width,
            height: header.height,
            bit_depth: PngBitDepth::from_u8(header.bit_depth)
                .expect("bug: validated bit depth must map to PngBitDepth"),
            color_mode: color_mode_from_color_type(header.color_type),
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
        let (header, ancillary, idat_data) = parse_png(bytes)?;
        let expected_filtered = expected_filtered_len(&header)?;
        expected_raw_len(&header)?;
        let filtered = decompress_zlib(&idat_data)?;
        if filtered.len() != expected_filtered {
            return Err(Error::InvalidData(
                format!(
                    "unexpected filtered data size: expected {}, got {}",
                    expected_filtered,
                    filtered.len()
                )
                .into(),
            ));
        }
        let pixels = decode_to_pixels(&header, &filtered, &ancillary)?;
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
        let encoded =
            EncodedImage::from_pixels(self.width, self.height, &self.pixels, self.encoding)?;
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

#[derive(Debug, Clone, Copy)]
struct PngHeader {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace_method: u8,
}

impl PngHeader {
    fn parse(chunk_data: &[u8]) -> Result<Self> {
        if chunk_data.len() != 13 {
            return Err(Error::InvalidData(
                "IHDR chunk must contain 13 bytes".into(),
            ));
        }
        let width = u32::from_be_bytes(
            chunk_data[0..4]
                .try_into()
                .expect("bug: IHDR width must be 4 bytes"),
        );
        let height = u32::from_be_bytes(
            chunk_data[4..8]
                .try_into()
                .expect("bug: IHDR height must be 4 bytes"),
        );
        if width == 0 || height == 0 {
            return Err(Error::InvalidData(
                "image dimensions must be non-zero".into(),
            ));
        }

        let header = Self {
            width,
            height,
            bit_depth: chunk_data[8],
            color_type: chunk_data[9],
            compression_method: chunk_data[10],
            filter_method: chunk_data[11],
            interlace_method: chunk_data[12],
        };
        header.validate()?;
        Ok(header)
    }

    fn validate(&self) -> Result<()> {
        if self.compression_method != 0 {
            return Err(Error::Unsupported(
                format!(
                    "unsupported compression method: {}",
                    self.compression_method
                )
                .into(),
            ));
        }
        if self.filter_method != 0 {
            return Err(Error::Unsupported(
                format!("unsupported filter method: {}", self.filter_method).into(),
            ));
        }
        if self.interlace_method > 1 {
            return Err(Error::Unsupported(
                format!("unsupported interlace method: {}", self.interlace_method).into(),
            ));
        }
        match (self.color_type, self.bit_depth) {
            (0, 1 | 2 | 4 | 8 | 16)
            | (3, 1 | 2 | 4 | 8)
            | (2, 8 | 16)
            | (4, 8 | 16)
            | (6, 8 | 16) => Ok(()),
            _ => Err(Error::Unsupported(
                format!(
                    "unsupported color type/bit depth combination: color_type={}, bit_depth={}",
                    self.color_type, self.bit_depth
                )
                .into(),
            )),
        }
    }

    fn samples_per_pixel(&self) -> usize {
        match self.color_type {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            6 => 4,
            _ => unreachable!(),
        }
    }

    fn bits_per_pixel(&self) -> usize {
        self.samples_per_pixel() * usize::from(self.bit_depth)
    }

    fn bytes_per_pixel(&self) -> usize {
        self.bits_per_pixel().div_ceil(8)
    }

    fn filter_bpp(&self) -> usize {
        if self.bit_depth < 8 {
            1
        } else {
            self.bytes_per_pixel()
        }
    }
}

fn parse_png_header(bytes: &[u8]) -> Result<PngHeader> {
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(Error::InvalidData("invalid PNG signature".into()));
    }
    let mut cursor = Cursor::new(&bytes[PNG_SIGNATURE.len()..]);
    let length = cursor.read_u32()? as usize;
    let chunk_type = cursor.read_array::<4>()?;
    if chunk_type != *b"IHDR" {
        return Err(Error::InvalidData("missing IHDR chunk".into()));
    }
    let chunk_data = cursor.read_bytes(length)?;
    PngHeader::parse(chunk_data)
}

#[derive(Debug, Clone)]
enum Transparency {
    Grayscale(u16),
    Truecolor([u16; 3]),
    Palette(Vec<u8>),
}

#[derive(Debug, Clone, Default)]
struct AncillaryChunks {
    palette: Option<Vec<[u8; 3]>>,
    transparency: Option<Transparency>,
}

impl AncillaryChunks {
    fn set_palette(&mut self, palette: Vec<[u8; 3]>) -> Result<()> {
        if self.palette.is_some() {
            return Err(Error::InvalidData("duplicate PLTE chunk".into()));
        }
        self.palette = Some(palette);
        Ok(())
    }

    fn set_transparency(&mut self, transparency: Transparency) -> Result<()> {
        if self.transparency.is_some() {
            return Err(Error::InvalidData("duplicate tRNS chunk".into()));
        }
        self.transparency = Some(transparency);
        Ok(())
    }

    fn validate(&self, header: &PngHeader) -> Result<()> {
        if header.color_type == 3 && self.palette.is_none() {
            return Err(Error::InvalidData("missing PLTE for palette image".into()));
        }
        if matches!(header.color_type, 0 | 4) && self.palette.is_some() {
            return Err(Error::InvalidData(
                "PLTE chunk is not allowed for grayscale images".into(),
            ));
        }
        match (&self.transparency, header.color_type) {
            (Some(Transparency::Grayscale(_)), 0) => {}
            (Some(Transparency::Truecolor(_)), 2) => {}
            (Some(Transparency::Palette(alpha)), 3) => {
                let palette_len = self.palette.as_ref().map_or(0, Vec::len);
                if alpha.len() > palette_len {
                    return Err(Error::InvalidData(
                        "tRNS length exceeds palette length".into(),
                    ));
                }
            }
            (Some(_), _) => {
                return Err(Error::InvalidData(
                    format!("tRNS is not allowed for color type {}", header.color_type).into(),
                ));
            }
            (None, _) => {}
        }
        Ok(())
    }
}

fn parse_palette(chunk_data: &[u8]) -> Result<Vec<[u8; 3]>> {
    if chunk_data.is_empty() || !chunk_data.len().is_multiple_of(3) {
        return Err(Error::InvalidData(
            "PLTE length must be a non-zero multiple of 3".into(),
        ));
    }
    let (palette_chunks, remainder) = chunk_data.as_chunks::<3>();
    debug_assert!(remainder.is_empty());
    let palette = palette_chunks.to_vec();
    if palette.len() > 256 {
        return Err(Error::InvalidData(
            "PLTE must not contain more than 256 entries".into(),
        ));
    }
    Ok(palette)
}

fn parse_transparency(
    chunk_data: &[u8],
    header: &PngHeader,
    ancillary: &AncillaryChunks,
) -> Result<Transparency> {
    match header.color_type {
        0 => {
            if chunk_data.len() != 2 {
                return Err(Error::InvalidData(
                    "grayscale tRNS chunk must contain 2 bytes".into(),
                ));
            }
            let sample = u16::from_be_bytes([chunk_data[0], chunk_data[1]]);
            let max = if header.bit_depth == 16 {
                u16::MAX
            } else {
                (1u16 << header.bit_depth) - 1
            };
            if sample > max {
                return Err(Error::InvalidData(
                    "invalid grayscale transparency sample".into(),
                ));
            }
            Ok(Transparency::Grayscale(sample))
        }
        2 => {
            if chunk_data.len() != 6 {
                return Err(Error::InvalidData(
                    "truecolor tRNS chunk must contain 6 bytes".into(),
                ));
            }
            Ok(Transparency::Truecolor([
                u16::from_be_bytes([chunk_data[0], chunk_data[1]]),
                u16::from_be_bytes([chunk_data[2], chunk_data[3]]),
                u16::from_be_bytes([chunk_data[4], chunk_data[5]]),
            ]))
        }
        3 => {
            if ancillary.palette.is_none() {
                return Err(Error::InvalidData(
                    "tRNS chunk must appear after PLTE".into(),
                ));
            }
            Ok(Transparency::Palette(chunk_data.to_vec()))
        }
        _ => Err(Error::InvalidData(
            format!("tRNS is not allowed for color type {}", header.color_type).into(),
        )),
    }
}

fn parse_png(bytes: &[u8]) -> Result<(PngHeader, AncillaryChunks, Vec<u8>)> {
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(Error::InvalidData("invalid PNG signature".into()));
    }

    let mut cursor = Cursor::new(&bytes[PNG_SIGNATURE.len()..]);
    let mut header = None;
    let mut idat_data = Vec::new();
    let mut ancillary = AncillaryChunks::default();
    let mut seen_idat = false;
    let mut seen_iend = false;

    while cursor.remaining() > 0 {
        let length = cursor.read_u32()? as usize;
        let chunk_type = cursor.read_array::<4>()?;
        let chunk_data = cursor.read_bytes(length)?;
        let expected_crc = cursor.read_u32()?;

        let actual_crc = crc::calculate(&[&chunk_type[..], chunk_data]);
        if actual_crc != expected_crc {
            return Err(Error::InvalidData(
                format!(
                    "CRC mismatch for chunk {}",
                    core::str::from_utf8(&chunk_type).unwrap_or("????"),
                )
                .into(),
            ));
        }

        match &chunk_type {
            b"IHDR" => {
                if header.is_some() {
                    return Err(Error::InvalidData("duplicate IHDR chunk".into()));
                }
                if seen_idat {
                    return Err(Error::InvalidData("IHDR chunk after IDAT".into()));
                }
                header = Some(PngHeader::parse(chunk_data)?);
            }
            b"PLTE" => {
                let Some(header) = header else {
                    return Err(Error::InvalidData("PLTE chunk before IHDR".into()));
                };
                if seen_idat {
                    return Err(Error::InvalidData("PLTE appears after IDAT".into()));
                }
                if matches!(header.color_type, 0 | 4) {
                    return Err(Error::InvalidData(
                        "PLTE chunk is not allowed for grayscale images".into(),
                    ));
                }
                ancillary.set_palette(parse_palette(chunk_data)?)?;
            }
            b"tRNS" => {
                let Some(header) = header else {
                    return Err(Error::InvalidData("tRNS chunk before IHDR".into()));
                };
                if seen_idat {
                    return Err(Error::InvalidData("tRNS appears after IDAT".into()));
                }
                ancillary.set_transparency(parse_transparency(chunk_data, &header, &ancillary)?)?;
            }
            b"IDAT" => {
                if header.is_none() {
                    return Err(Error::InvalidData("IDAT chunk before IHDR".into()));
                }
                seen_idat = true;
                idat_data.extend_from_slice(chunk_data);
            }
            b"IEND" => {
                seen_iend = true;
                break;
            }
            _ => {}
        }
    }

    if !seen_iend {
        return Err(Error::InvalidData("missing IEND chunk".into()));
    }
    let header = header.ok_or_else(|| Error::InvalidData("missing IHDR chunk".into()))?;
    if idat_data.is_empty() {
        return Err(Error::InvalidData("missing IDAT chunk".into()));
    }
    ancillary.validate(&header)?;
    Ok((header, ancillary, idat_data))
}

fn decompress_zlib(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 6 {
        return Err(Error::InvalidData("zlib stream is too short".into()));
    }

    let cmf = data[0];
    let flg = data[1];
    let header = u16::from(cmf) << 8 | u16::from(flg);
    if header % 31 != 0 {
        return Err(Error::InvalidData(
            "zlib header check bits are invalid".into(),
        ));
    }
    if cmf & 0x0F != 8 {
        return Err(Error::Unsupported(
            format!("unsupported zlib compression method: {}", cmf & 0x0F).into(),
        ));
    }
    if cmf >> 4 > 7 {
        return Err(Error::Unsupported("zlib window size is too large".into()));
    }
    if (flg & 0x20) != 0 {
        return Err(Error::Unsupported(
            "zlib preset dictionary is not supported".into(),
        ));
    }

    let deflate_bytes = &data[2..data.len() - 4];
    let decoded = deflate::decompress(deflate_bytes)
        .map_err(|error| Error::InvalidData(format!("invalid deflate stream: {error}").into()))?;
    let expected_adler = u32::from_be_bytes(
        data[data.len() - 4..]
            .try_into()
            .expect("bug: zlib trailer must be 4 bytes"),
    );
    let actual_adler = adler32::calculate(&decoded);
    if actual_adler != expected_adler {
        return Err(Error::InvalidData("zlib adler32 checksum mismatch".into()));
    }
    Ok(decoded)
}

fn decode_to_pixels(
    header: &PngHeader,
    filtered: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<PngPixels<'static>> {
    if header.interlace_method == 0 {
        let raw = unfilter_scanlines(header, header.width, header.height, filtered)?;
        convert_to_pixels(header, header.width, &raw, ancillary)
    } else {
        decode_adam7_to_pixels(header, filtered, ancillary)
    }
}

fn convert_to_pixels(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<PngPixels<'static>> {
    match (header.color_type, header.bit_depth) {
        (0, 1) => convert_grayscale_low_bit_to_pixels(header, width, raw, ancillary, 1),
        (0, 2) => convert_grayscale_low_bit_to_pixels(header, width, raw, ancillary, 2),
        (0, 4) => convert_grayscale_low_bit_to_pixels(header, width, raw, ancillary, 4),
        (0, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            if let Some(transparent) = transparent {
                let mut data = Vec::with_capacity(raw.len() * 2);
                for &gray in raw {
                    data.extend_from_slice(&[
                        gray,
                        if u16::from(gray) == transparent {
                            0
                        } else {
                            255
                        },
                    ]);
                }
                Ok(PngPixels::GrayAlpha8(Cow::Owned(data)))
            } else {
                Ok(PngPixels::Gray8(Cow::Owned(raw.to_vec())))
            }
        }
        (0, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            let samples = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes(*chunk))
                .collect::<Vec<_>>();
            if let Some(transparent) = transparent {
                let mut data = Vec::with_capacity(samples.len() * 2);
                for gray in samples {
                    data.extend_from_slice(&[gray, if gray == transparent { 0 } else { u16::MAX }]);
                }
                Ok(PngPixels::GrayAlpha16(Cow::Owned(data)))
            } else {
                Ok(PngPixels::Gray16(Cow::Owned(samples)))
            }
        }
        (2, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            if let Some(transparent) = transparent {
                let mut data = Vec::with_capacity(raw.len() / 3 * 4);
                let (pixels, remainder) = raw.as_chunks::<3>();
                debug_assert!(remainder.is_empty());
                for &[r, g, b] in pixels {
                    let alpha = if [u16::from(r), u16::from(g), u16::from(b)] == transparent {
                        0
                    } else {
                        255
                    };
                    data.extend_from_slice(&[r, g, b, alpha]);
                }
                Ok(PngPixels::Rgba8(Cow::Owned(data)))
            } else {
                Ok(PngPixels::Rgb8(Cow::Owned(raw.to_vec())))
            }
        }
        (2, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            let samples = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes(*chunk))
                .collect::<Vec<_>>();
            if let Some(transparent) = transparent {
                let mut data = Vec::with_capacity(samples.len() / 3 * 4);
                let (pixels, remainder) = samples.as_chunks::<3>();
                debug_assert!(remainder.is_empty());
                for &[r, g, b] in pixels {
                    let alpha = if [r, g, b] == transparent {
                        0
                    } else {
                        u16::MAX
                    };
                    data.extend_from_slice(&[r, g, b, alpha]);
                }
                Ok(PngPixels::Rgba16(Cow::Owned(data)))
            } else {
                Ok(PngPixels::Rgb16(Cow::Owned(samples)))
            }
        }
        (3, 1 | 2 | 4 | 8) => convert_indexed_to_pixels(header, width, raw, ancillary),
        (4, 8) => Ok(PngPixels::GrayAlpha8(Cow::Owned(raw.to_vec()))),
        (4, 16) => Ok(PngPixels::GrayAlpha16(Cow::Owned(
            raw.as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes(*chunk))
                .collect(),
        ))),
        (6, 8) => Ok(PngPixels::Rgba8(Cow::Owned(raw.to_vec()))),
        (6, 16) => Ok(PngPixels::Rgba16(Cow::Owned(
            raw.as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes(*chunk))
                .collect(),
        ))),
        _ => unreachable!(),
    }
}

fn convert_grayscale_low_bit_to_pixels(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
    bit_depth: u8,
) -> Result<PngPixels<'static>> {
    let transparent = match ancillary.transparency {
        Some(Transparency::Grayscale(value)) => Some(value),
        _ => None,
    };
    let row_stride = packed_stride_for_width(header, width)?;
    let mut unpacked = Vec::with_capacity(width as usize * (raw.len() / row_stride.max(1)));
    for row in raw.chunks_exact(row_stride) {
        unpacked.extend(unpack_samples(row, width as usize, header.bit_depth));
    }
    if let Some(transparent) = transparent {
        let mut data = Vec::with_capacity(unpacked.len() * 2);
        for sample in unpacked {
            let gray = scale_sample_to_u8(u16::from(sample), bit_depth);
            let alpha = if u16::from(sample) == transparent {
                0
            } else {
                255
            };
            data.extend_from_slice(&[gray, alpha]);
        }
        Ok(PngPixels::GrayAlpha8(Cow::Owned(data)))
    } else {
        Ok(match bit_depth {
            1 => PngPixels::Gray1(Cow::Owned(unpacked)),
            2 => PngPixels::Gray2(Cow::Owned(unpacked)),
            4 => PngPixels::Gray4(Cow::Owned(unpacked)),
            _ => unreachable!(),
        })
    }
}

fn convert_indexed_to_pixels(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<PngPixels<'static>> {
    let palette = ancillary
        .palette
        .as_ref()
        .ok_or_else(|| Error::InvalidData("missing PLTE for palette image".into()))?;
    let trns = match ancillary.transparency.as_ref() {
        Some(Transparency::Palette(alpha)) => Some(alpha.clone()),
        _ => None,
    };
    let row_stride = packed_stride_for_width(header, width)?;
    let mut unpacked = Vec::with_capacity(width as usize * (raw.len() / row_stride.max(1)));
    for row in raw.chunks_exact(row_stride) {
        unpacked.extend(unpack_samples(row, width as usize, header.bit_depth));
    }
    Ok(match header.bit_depth {
        1 => PngPixels::Indexed1 {
            indices: Cow::Owned(unpacked),
            palette: Cow::Owned(flatten_palette(palette)),
            trns: trns.map(Cow::Owned),
        },
        2 => PngPixels::Indexed2 {
            indices: Cow::Owned(unpacked),
            palette: Cow::Owned(flatten_palette(palette)),
            trns: trns.map(Cow::Owned),
        },
        4 => PngPixels::Indexed4 {
            indices: Cow::Owned(unpacked),
            palette: Cow::Owned(flatten_palette(palette)),
            trns: trns.map(Cow::Owned),
        },
        8 => PngPixels::Indexed8 {
            indices: Cow::Owned(unpacked),
            palette: Cow::Owned(flatten_palette(palette)),
            trns: trns.map(Cow::Owned),
        },
        _ => unreachable!(),
    })
}

fn expected_filtered_len(header: &PngHeader) -> Result<usize> {
    if header.interlace_method == 0 {
        expected_filtered_len_for_size(header, header.width, header.height)
    } else {
        let mut total = 0usize;
        for pass in ADAM7_PASSES {
            let pass_width = adam7_axis_size(header.width, pass.x_start, pass.x_step);
            let pass_height = adam7_axis_size(header.height, pass.y_start, pass.y_step);
            if pass_width == 0 || pass_height == 0 {
                continue;
            }
            total = total
                .checked_add(expected_filtered_len_for_size(
                    header,
                    pass_width,
                    pass_height,
                )?)
                .ok_or_else(|| Error::InvalidData("filtered data size overflow".into()))?;
        }
        Ok(total)
    }
}

fn expected_filtered_len_for_size(header: &PngHeader, width: u32, height: u32) -> Result<usize> {
    let stride = packed_stride_for_width(header, width)?;
    (stride + 1)
        .checked_mul(height as usize)
        .ok_or_else(|| Error::InvalidData("filtered data size overflow".into()))
}

fn expected_raw_len(header: &PngHeader) -> Result<usize> {
    let stride = packed_stride_for_width(header, header.width)?;
    stride
        .checked_mul(header.height as usize)
        .ok_or_else(|| Error::InvalidData("raw image size overflow".into()))
}

fn unfilter_scanlines(
    header: &PngHeader,
    width: u32,
    height: u32,
    filtered: &[u8],
) -> Result<Vec<u8>> {
    let stride = packed_stride_for_width(header, width)?;
    let expected_len = (stride + 1)
        .checked_mul(height as usize)
        .ok_or_else(|| Error::InvalidData("filtered data size overflow".into()))?;
    if filtered.len() != expected_len {
        return Err(Error::InvalidData(
            format!(
                "unexpected filtered data size: expected {}, got {}",
                expected_len,
                filtered.len()
            )
            .into(),
        ));
    }

    let bpp = header.filter_bpp();
    let mut raw = vec![0; stride * height as usize];
    for row in 0..height as usize {
        let filter = filtered[row * (stride + 1)];
        let src = &filtered[row * (stride + 1) + 1..(row + 1) * (stride + 1)];
        let row_start = row * stride;
        let (before, current_and_after) = raw.split_at_mut(row_start);
        let dst = &mut current_and_after[..stride];
        let prev: Option<&[u8]> = if row == 0 {
            None
        } else {
            Some(&before[before.len() - stride..])
        };
        match filter {
            0 => dst.copy_from_slice(src),
            1 => {
                for i in 0..stride {
                    let left = if i >= bpp { dst[i - bpp] } else { 0 };
                    dst[i] = src[i].wrapping_add(left);
                }
            }
            2 => {
                for i in 0..stride {
                    let up = prev.map_or(0, |row| row[i]);
                    dst[i] = src[i].wrapping_add(up);
                }
            }
            3 => {
                for i in 0..stride {
                    let left = if i >= bpp { dst[i - bpp] } else { 0 };
                    let up = prev.map_or(0, |row| row[i]);
                    dst[i] = src[i].wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8);
                }
            }
            4 => {
                for i in 0..stride {
                    let left = if i >= bpp { dst[i - bpp] } else { 0 };
                    let up = prev.map_or(0, |row| row[i]);
                    let up_left = if i >= bpp {
                        prev.map_or(0, |row| row[i - bpp])
                    } else {
                        0
                    };
                    dst[i] = src[i].wrapping_add(paeth_predictor(left, up, up_left));
                }
            }
            _ => {
                return Err(Error::InvalidData(
                    format!("unsupported PNG filter type: {}", filter).into(),
                ));
            }
        }
    }
    Ok(raw)
}

fn convert_to_rgba16(
    header: &PngHeader,
    width: u32,
    height: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<Vec<u16>> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    match (header.color_type, header.bit_depth) {
        (0, 1 | 2 | 4) => {
            let row_stride = packed_stride_for_width(header, width)?;
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            for row in raw.chunks_exact(row_stride) {
                for sample in unpack_samples(row, width as usize, header.bit_depth) {
                    let gray =
                        upscale_u8_to_u16(scale_sample_to_u8(u16::from(sample), header.bit_depth));
                    let alpha = if Some(u16::from(sample)) == transparent {
                        0
                    } else {
                        u16::MAX
                    };
                    rgba.extend_from_slice(&[gray, gray, gray, alpha]);
                }
            }
        }
        (0, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            for &gray in raw {
                let gray = upscale_u8_to_u16(gray);
                let alpha = if Some(u16::from(downsample_u16(gray))) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        (0, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            let (pixels, remainder) = raw.as_chunks::<2>();
            debug_assert!(remainder.is_empty());
            for chunk in pixels {
                let gray = u16::from_be_bytes(*chunk);
                let alpha = if Some(gray) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        (2, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            let (pixels, remainder) = raw.as_chunks::<3>();
            debug_assert!(remainder.is_empty());
            for &[r, g, b] in pixels {
                let rgb = [u16::from(r), u16::from(g), u16::from(b)];
                let alpha = if Some(rgb) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(&[
                    upscale_u8_to_u16(r),
                    upscale_u8_to_u16(g),
                    upscale_u8_to_u16(b),
                    alpha,
                ]);
            }
        }
        (2, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            let (pixels, remainder) = raw.as_chunks::<6>();
            debug_assert!(remainder.is_empty());
            for &[r0, r1, g0, g1, b0, b1] in pixels {
                let rgb = [
                    u16::from_be_bytes([r0, r1]),
                    u16::from_be_bytes([g0, g1]),
                    u16::from_be_bytes([b0, b1]),
                ];
                let alpha = if Some(rgb) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
        }
        (3, 1 | 2 | 4 | 8) => {
            let palette_pixels = convert_indexed_to_pixels(header, width, raw, ancillary)?;
            rgba.extend(palette_pixels.to_rgba16_vec());
        }
        (4, 8) => {
            let (pixels, remainder) = raw.as_chunks::<2>();
            debug_assert!(remainder.is_empty());
            for &[gray, alpha] in pixels {
                let gray = upscale_u8_to_u16(gray);
                let alpha = upscale_u8_to_u16(alpha);
                rgba.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        (4, 16) => {
            let (pixels, remainder) = raw.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            for &[g0, g1, a0, a1] in pixels {
                let gray = u16::from_be_bytes([g0, g1]);
                let alpha = u16::from_be_bytes([a0, a1]);
                rgba.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        (6, 8) => {
            let (pixels, remainder) = raw.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            for &[r, g, b, a] in pixels {
                rgba.extend_from_slice(&[
                    upscale_u8_to_u16(r),
                    upscale_u8_to_u16(g),
                    upscale_u8_to_u16(b),
                    upscale_u8_to_u16(a),
                ]);
            }
        }
        (6, 16) => {
            let (pixels, remainder) = raw.as_chunks::<8>();
            debug_assert!(remainder.is_empty());
            for &[r0, r1, g0, g1, b0, b1, a0, a1] in pixels {
                rgba.extend_from_slice(&[
                    u16::from_be_bytes([r0, r1]),
                    u16::from_be_bytes([g0, g1]),
                    u16::from_be_bytes([b0, b1]),
                    u16::from_be_bytes([a0, a1]),
                ]);
            }
        }
        _ => unreachable!(),
    }
    Ok(rgba)
}

fn expected_rgba16_len(header: &PngHeader) -> Result<usize> {
    (header.width as usize)
        .checked_mul(header.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| Error::InvalidData("decoded RGBA16 size overflow".into()))
}

fn pixels_from_rgba16_source(
    header: &PngHeader,
    ancillary: &AncillaryChunks,
    rgba: &[u16],
) -> PngPixels<'static> {
    match (
        header.color_type,
        header.bit_depth,
        ancillary.transparency.is_some(),
    ) {
        (0, 16, false) => {
            let (pixels, remainder) = rgba.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let data = pixels
                .iter()
                .map(|[gray, _, _, _]| *gray)
                .collect::<Vec<_>>();
            PngPixels::Gray16(Cow::Owned(data))
        }
        (0, _, true) => {
            if header.bit_depth == 16 {
                let mut data = Vec::with_capacity(rgba.len() / 2);
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                for &[gray, _, _, alpha] in pixels {
                    data.extend_from_slice(&[gray, alpha]);
                }
                PngPixels::GrayAlpha16(Cow::Owned(data))
            } else {
                let mut data = Vec::with_capacity(rgba.len() / 2);
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                for &[gray, _, _, alpha] in pixels {
                    data.extend_from_slice(&[downsample_u16(gray), downsample_u16(alpha)]);
                }
                PngPixels::GrayAlpha8(Cow::Owned(data))
            }
        }
        (2, 16, false) => {
            let (pixels, remainder) = rgba.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let data = pixels
                .iter()
                .flat_map(|[r, g, b, _]| [*r, *g, *b])
                .collect::<Vec<_>>();
            PngPixels::Rgb16(Cow::Owned(data))
        }
        (2, _, true) => {
            if header.bit_depth == 16 {
                PngPixels::Rgba16(Cow::Owned(rgba.to_vec()))
            } else {
                PngPixels::Rgba8(Cow::Owned(
                    rgba.iter().copied().map(downsample_u16).collect(),
                ))
            }
        }
        (4, 8, _) => {
            let mut data = Vec::with_capacity(rgba.len() / 2);
            let (pixels, remainder) = rgba.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            for &[gray, _, _, alpha] in pixels {
                data.extend_from_slice(&[downsample_u16(gray), downsample_u16(alpha)]);
            }
            PngPixels::GrayAlpha8(Cow::Owned(data))
        }
        (4, 16, _) => {
            let mut data = Vec::with_capacity(rgba.len() / 2);
            let (pixels, remainder) = rgba.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            for &[gray, _, _, alpha] in pixels {
                data.extend_from_slice(&[gray, alpha]);
            }
            PngPixels::GrayAlpha16(Cow::Owned(data))
        }
        (6, 8, _) => PngPixels::Rgba8(Cow::Owned(
            rgba.iter().copied().map(downsample_u16).collect(),
        )),
        (6, 16, _) => PngPixels::Rgba16(Cow::Owned(rgba.to_vec())),
        _ => {
            let (pixels, remainder) = rgba.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let data = pixels
                .iter()
                .map(|[gray, _, _, _]| downsample_u16(*gray))
                .collect::<Vec<_>>();
            match header.bit_depth {
                1 => PngPixels::Gray1(Cow::Owned(
                    data.iter().copied().map(|value| value / 255).collect(),
                )),
                2 => PngPixels::Gray2(Cow::Owned(
                    data.iter().copied().map(|value| value / 85).collect(),
                )),
                4 => PngPixels::Gray4(Cow::Owned(
                    data.iter().copied().map(|value| value / 17).collect(),
                )),
                8 => PngPixels::Gray8(Cow::Owned(data)),
                _ => unreachable!(),
            }
        }
    }
}

fn unpack_samples(bytes: &[u8], width: usize, bit_depth: u8) -> impl Iterator<Item = u8> + '_ {
    let mask = (1u16 << bit_depth) - 1;
    (0..width).map(move |pixel| {
        let bit_offset = pixel * usize::from(bit_depth);
        let byte = bytes[bit_offset / 8];
        let shift = 8 - usize::from(bit_depth) - (bit_offset % 8);
        ((u16::from(byte) >> shift) & mask) as u8
    })
}

#[derive(Debug)]
struct EncodedImage {
    bit_depth: u8,
    color_type: u8,
    interlace_method: u8,
    filtered_data: Vec<u8>,
    palette: Option<Vec<[u8; 3]>>,
    trns: Option<Vec<u8>>,
}

impl EncodedImage {
    fn from_pixels(
        width: u32,
        height: u32,
        pixels: &PngPixels<'_>,
        encoding: PngEncoding,
    ) -> Result<Self> {
        if encoding.bit_depth == PngBitDepth::Sixteen {
            let rgba16 = pixels.to_rgba16_vec();
            return Self::from_rgba16(width, height, &rgba16, encoding);
        }
        let rgba8 = pixels.to_rgba8_vec();
        Self::from_rgba(width, height, &rgba8, encoding)
    }

    fn from_rgba(width: u32, height: u32, rgba: &[u8], encoding: PngEncoding) -> Result<Self> {
        let (pixels, remainder) = rgba.as_chunks::<4>();
        debug_assert!(remainder.is_empty());
        let pixels = pixels.to_vec();
        let target = EncodingTarget::analyze(&pixels, encoding)?;
        let interlace_method = u8::from(encoding.interlaced);
        let filtered_data = if encoding.interlaced {
            build_adam7_filtered_data(width, height, &pixels, &target)?
        } else {
            build_filtered_data(width, height, &pixels, &target)?
        };
        Ok(Self {
            bit_depth: target.bit_depth,
            color_type: target.color_type,
            interlace_method,
            filtered_data,
            palette: target.palette,
            trns: target.trns,
        })
    }

    fn from_rgba16(width: u32, height: u32, rgba: &[u16], encoding: PngEncoding) -> Result<Self> {
        let (pixels, remainder) = rgba.as_chunks::<4>();
        debug_assert!(remainder.is_empty());
        let pixels = pixels.to_vec();
        let target = EncodingTarget16::analyze(&pixels, encoding)?;
        let interlace_method = u8::from(encoding.interlaced);
        let filtered_data = if encoding.interlaced {
            build_adam7_filtered_data16(width, height, &pixels, &target)?
        } else {
            build_filtered_data16(width, height, &pixels, &target)?
        };
        Ok(Self {
            bit_depth: 16,
            color_type: target.color_type,
            interlace_method,
            filtered_data,
            palette: None,
            trns: None,
        })
    }
}

#[derive(Debug)]
struct EncodingTarget {
    bit_depth: u8,
    color_type: u8,
    palette: Option<Vec<[u8; 3]>>,
    trns: Option<Vec<u8>>,
    pixel_kind: EncodedPixelKind,
}

#[derive(Debug)]
enum EncodedPixelKind {
    GrayscalePacked,
    Grayscale8,
    GrayscaleAlpha8,
    Rgb8,
    Rgba8,
    Indexed,
}

#[derive(Debug)]
struct EncodingTarget16 {
    color_type: u8,
    pixel_kind: EncodedPixelKind16,
}

#[derive(Debug)]
enum EncodedPixelKind16 {
    Grayscale16,
    GrayscaleAlpha16,
    Rgb16,
    Rgba16,
}

impl EncodingTarget {
    fn analyze(pixels: &[[u8; 4]], encoding: PngEncoding) -> Result<Self> {
        let effective_bit_depth = encoding.bit_depth.effective_for_rgba8();
        match encoding.color_mode {
            PngColorMode::Grayscale => {
                if !pixels_are_opaque_grayscale(pixels) {
                    return Err(Error::Unsupported(
                        "grayscale encoding requires opaque grayscale pixels".into(),
                    ));
                }
                validate_grayscale_bit_depth(pixels, effective_bit_depth)?;
                Ok(Self {
                    bit_depth: effective_bit_depth.as_u8(),
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE,
                    palette: None,
                    trns: None,
                    pixel_kind: if effective_bit_depth.as_u8() < 8 {
                        EncodedPixelKind::GrayscalePacked
                    } else {
                        EncodedPixelKind::Grayscale8
                    },
                })
            }
            PngColorMode::GrayscaleAlpha => {
                if !pixels_are_grayscale(pixels) {
                    return Err(Error::Unsupported(
                        "grayscale+alpha encoding requires grayscale pixels".into(),
                    ));
                }
                validate_exact_bit_depth(
                    PngColorMode::GrayscaleAlpha,
                    effective_bit_depth,
                    &[PngBitDepth::Eight],
                )?;
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE_ALPHA,
                    palette: None,
                    trns: None,
                    pixel_kind: EncodedPixelKind::GrayscaleAlpha8,
                })
            }
            PngColorMode::Rgb => {
                if !pixels_are_opaque(pixels) {
                    return Err(Error::Unsupported(
                        "rgb encoding requires opaque pixels".into(),
                    ));
                }
                validate_exact_bit_depth(
                    PngColorMode::Rgb,
                    effective_bit_depth,
                    &[PngBitDepth::Eight],
                )?;
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_RGB,
                    palette: None,
                    trns: None,
                    pixel_kind: EncodedPixelKind::Rgb8,
                })
            }
            PngColorMode::Rgba => {
                validate_exact_bit_depth(
                    PngColorMode::Rgba,
                    effective_bit_depth,
                    &[PngBitDepth::Eight],
                )?;
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_RGBA,
                    palette: None,
                    trns: None,
                    pixel_kind: EncodedPixelKind::Rgba8,
                })
            }
            PngColorMode::Indexed => {
                let Some(indexed) = analyze_palette(pixels) else {
                    return Err(Error::Unsupported(
                        "indexed encoding requires at most 256 distinct colors".into(),
                    ));
                };
                validate_indexed_bit_depth(indexed.palette.len(), effective_bit_depth)?;
                Ok(Self {
                    bit_depth: effective_bit_depth.as_u8(),
                    color_type: IhdrChunk::COLOR_TYPE_INDEXED,
                    palette: Some(indexed.palette),
                    trns: indexed.trns,
                    pixel_kind: EncodedPixelKind::Indexed,
                })
            }
        }
    }
}

impl EncodingTarget16 {
    fn analyze(pixels: &[[u16; 4]], encoding: PngEncoding) -> Result<Self> {
        validate_exact_bit_depth(
            encoding.color_mode,
            encoding.bit_depth,
            &[PngBitDepth::Sixteen],
        )?;
        match encoding.color_mode {
            PngColorMode::Grayscale => {
                if !pixels_are_opaque_grayscale16(pixels) {
                    return Err(Error::Unsupported(
                        "grayscale encoding requires opaque grayscale pixels".into(),
                    ));
                }
                Ok(Self {
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE,
                    pixel_kind: EncodedPixelKind16::Grayscale16,
                })
            }
            PngColorMode::GrayscaleAlpha => {
                if !pixels_are_grayscale16(pixels) {
                    return Err(Error::Unsupported(
                        "grayscale+alpha encoding requires grayscale pixels".into(),
                    ));
                }
                Ok(Self {
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE_ALPHA,
                    pixel_kind: EncodedPixelKind16::GrayscaleAlpha16,
                })
            }
            PngColorMode::Rgb => {
                if !pixels_are_opaque16(pixels) {
                    return Err(Error::Unsupported(
                        "rgb encoding requires opaque pixels".into(),
                    ));
                }
                Ok(Self {
                    color_type: IhdrChunk::COLOR_TYPE_RGB,
                    pixel_kind: EncodedPixelKind16::Rgb16,
                })
            }
            PngColorMode::Rgba => Ok(Self {
                color_type: IhdrChunk::COLOR_TYPE_RGBA,
                pixel_kind: EncodedPixelKind16::Rgba16,
            }),
            PngColorMode::Indexed => Err(Error::Unsupported(
                "16-bit indexed encoding is not supported".into(),
            )),
        }
    }
}

#[derive(Debug)]
struct IndexedAnalysis {
    palette: Vec<[u8; 3]>,
    trns: Option<Vec<u8>>,
}

fn color_mode_from_color_type(color_type: u8) -> PngColorMode {
    match color_type {
        0 => PngColorMode::Grayscale,
        2 => PngColorMode::Rgb,
        3 => PngColorMode::Indexed,
        4 => PngColorMode::GrayscaleAlpha,
        6 => PngColorMode::Rgba,
        _ => unreachable!(),
    }
}

fn color_type_from_color_mode(color_mode: PngColorMode) -> u8 {
    match color_mode {
        PngColorMode::Grayscale => 0,
        PngColorMode::Rgb => 2,
        PngColorMode::Indexed => 3,
        PngColorMode::GrayscaleAlpha => 4,
        PngColorMode::Rgba => 6,
    }
}

fn pixels_are_opaque(pixels: &[[u8; 4]]) -> bool {
    pixels.iter().all(|pixel| pixel[3] == 255)
}

fn pixels_are_opaque16(pixels: &[[u16; 4]]) -> bool {
    pixels.iter().all(|pixel| pixel[3] == u16::MAX)
}

fn pixels_are_grayscale(pixels: &[[u8; 4]]) -> bool {
    pixels
        .iter()
        .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
}

fn pixels_are_grayscale16(pixels: &[[u16; 4]]) -> bool {
    pixels
        .iter()
        .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2])
}

fn pixels_are_opaque_grayscale(pixels: &[[u8; 4]]) -> bool {
    pixels_are_grayscale(pixels) && pixels_are_opaque(pixels)
}

fn pixels_are_opaque_grayscale16(pixels: &[[u16; 4]]) -> bool {
    pixels_are_grayscale16(pixels) && pixels_are_opaque16(pixels)
}

fn validate_exact_bit_depth(
    color_mode: PngColorMode,
    bit_depth: PngBitDepth,
    allowed: &[PngBitDepth],
) -> Result<()> {
    if allowed.contains(&bit_depth) {
        Ok(())
    } else {
        Err(Error::Unsupported(
            format!(
                "{color_mode:?} encoding does not support {}-bit output",
                bit_depth.as_u8()
            )
            .into(),
        ))
    }
}

fn validate_grayscale_bit_depth(pixels: &[[u8; 4]], bit_depth: PngBitDepth) -> Result<()> {
    validate_exact_bit_depth(
        PngColorMode::Grayscale,
        bit_depth,
        &[
            PngBitDepth::One,
            PngBitDepth::Two,
            PngBitDepth::Four,
            PngBitDepth::Eight,
        ],
    )?;
    if grayscale_pixels_fit_bit_depth(pixels, bit_depth) {
        Ok(())
    } else {
        Err(Error::Unsupported(
            format!(
                "grayscale pixels are not exactly representable at {}-bit",
                bit_depth.as_u8()
            )
            .into(),
        ))
    }
}

fn grayscale_pixels_fit_bit_depth(pixels: &[[u8; 4]], bit_depth: PngBitDepth) -> bool {
    match bit_depth {
        PngBitDepth::One => pixels.iter().all(|pixel| matches!(pixel[0], 0 | 255)),
        PngBitDepth::Two => pixels
            .iter()
            .all(|pixel| matches!(pixel[0], 0 | 85 | 170 | 255)),
        PngBitDepth::Four => pixels.iter().all(|pixel| pixel[0] % 17 == 0),
        PngBitDepth::Eight => true,
        PngBitDepth::Sixteen => false,
    }
}

fn validate_indexed_bit_depth(size: usize, bit_depth: PngBitDepth) -> Result<()> {
    validate_exact_bit_depth(
        PngColorMode::Indexed,
        bit_depth,
        &[
            PngBitDepth::One,
            PngBitDepth::Two,
            PngBitDepth::Four,
            PngBitDepth::Eight,
        ],
    )?;
    let capacity = match bit_depth {
        PngBitDepth::One => 2,
        PngBitDepth::Two => 4,
        PngBitDepth::Four => 16,
        PngBitDepth::Eight => 256,
        PngBitDepth::Sixteen => unreachable!(),
    };
    if size <= capacity {
        Ok(())
    } else {
        Err(Error::Unsupported(
            format!(
                "indexed palette of size {size} does not fit in {}-bit output",
                bit_depth.as_u8()
            )
            .into(),
        ))
    }
}

fn analyze_palette(pixels: &[[u8; 4]]) -> Option<IndexedAnalysis> {
    let mut map = BTreeMap::<[u8; 4], usize>::new();
    let mut palette = Vec::<[u8; 3]>::new();
    let mut alpha = Vec::<u8>::new();
    for &pixel in pixels {
        if map.contains_key(&pixel) {
            continue;
        }
        if palette.len() == 256 {
            return None;
        }
        map.insert(pixel, palette.len());
        palette.push([pixel[0], pixel[1], pixel[2]]);
        alpha.push(pixel[3]);
    }
    let trns = if alpha.iter().all(|&value| value == 255) {
        None
    } else {
        while alpha.last() == Some(&255) {
            alpha.pop();
        }
        Some(alpha)
    };
    Some(IndexedAnalysis { palette, trns })
}

fn build_filtered_data(
    width: u32,
    height: u32,
    pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<Vec<u8>> {
    let mut filtered = Vec::new();
    for row in 0..height as usize {
        filtered.push(0);
        let row_pixels = &pixels[row * width as usize..(row + 1) * width as usize];
        encode_row_into(&mut filtered, row_pixels, target)?;
    }
    Ok(filtered)
}

fn build_adam7_filtered_data(
    width: u32,
    height: u32,
    pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<Vec<u8>> {
    let mut filtered = Vec::new();
    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        for pass_y in 0..pass_height as usize {
            filtered.push(0);
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            let row_pixels = (0..pass_width as usize)
                .map(|pass_x| {
                    let x = pass.x_start as usize + pass_x * pass.x_step as usize;
                    pixels[y * width as usize + x]
                })
                .collect::<Vec<_>>();
            encode_row_into(&mut filtered, &row_pixels, target)?;
        }
    }
    Ok(filtered)
}

fn build_filtered_data16(
    width: u32,
    height: u32,
    pixels: &[[u16; 4]],
    target: &EncodingTarget16,
) -> Result<Vec<u8>> {
    let mut filtered = Vec::new();
    for row in 0..height as usize {
        filtered.push(0);
        let row_pixels = &pixels[row * width as usize..(row + 1) * width as usize];
        encode_row16_into(&mut filtered, row_pixels, target);
    }
    Ok(filtered)
}

fn build_adam7_filtered_data16(
    width: u32,
    height: u32,
    pixels: &[[u16; 4]],
    target: &EncodingTarget16,
) -> Result<Vec<u8>> {
    let mut filtered = Vec::new();
    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        for pass_y in 0..pass_height as usize {
            filtered.push(0);
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            let row_pixels = (0..pass_width as usize)
                .map(|pass_x| {
                    let x = pass.x_start as usize + pass_x * pass.x_step as usize;
                    pixels[y * width as usize + x]
                })
                .collect::<Vec<_>>();
            encode_row16_into(&mut filtered, &row_pixels, target);
        }
    }
    Ok(filtered)
}

fn encode_row_into(
    out: &mut Vec<u8>,
    row_pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<()> {
    match target.pixel_kind {
        EncodedPixelKind::GrayscalePacked => {
            let samples = row_pixels
                .iter()
                .map(|pixel| quantize_grayscale_sample(pixel[0], target.bit_depth))
                .collect::<Vec<_>>();
            pack_samples_to(out, &samples, target.bit_depth);
        }
        EncodedPixelKind::Grayscale8 => {
            out.extend(row_pixels.iter().map(|pixel| pixel[0]));
        }
        EncodedPixelKind::GrayscaleAlpha8 => {
            for pixel in row_pixels {
                out.extend_from_slice(&[pixel[0], pixel[3]]);
            }
        }
        EncodedPixelKind::Rgb8 => {
            for pixel in row_pixels {
                out.extend_from_slice(&pixel[..3]);
            }
        }
        EncodedPixelKind::Rgba8 => {
            for pixel in row_pixels {
                out.extend_from_slice(pixel);
            }
        }
        EncodedPixelKind::Indexed => {
            let palette = target
                .palette
                .as_ref()
                .expect("bug: indexed encoding target must include a palette");
            let indices = row_pixels
                .iter()
                .map(|pixel| {
                    palette
                        .iter()
                        .zip(target_alpha(target))
                        .position(|(rgb, alpha)| {
                            *rgb == [pixel[0], pixel[1], pixel[2]] && alpha == pixel[3]
                        })
                        .map(|index| index as u8)
                        .ok_or_else(|| {
                            Error::InvalidData("pixel missing from indexed palette".into())
                        })
                })
                .collect::<core::result::Result<Vec<_>, _>>()?;
            pack_samples_to(out, &indices, target.bit_depth);
        }
    }
    Ok(())
}

fn encode_row16_into(out: &mut Vec<u8>, row_pixels: &[[u16; 4]], target: &EncodingTarget16) {
    match target.pixel_kind {
        EncodedPixelKind16::Grayscale16 => {
            for pixel in row_pixels {
                out.extend_from_slice(&pixel[0].to_be_bytes());
            }
        }
        EncodedPixelKind16::GrayscaleAlpha16 => {
            for pixel in row_pixels {
                out.extend_from_slice(&pixel[0].to_be_bytes());
                out.extend_from_slice(&pixel[3].to_be_bytes());
            }
        }
        EncodedPixelKind16::Rgb16 => {
            for pixel in row_pixels {
                out.extend_from_slice(&pixel[0].to_be_bytes());
                out.extend_from_slice(&pixel[1].to_be_bytes());
                out.extend_from_slice(&pixel[2].to_be_bytes());
            }
        }
        EncodedPixelKind16::Rgba16 => {
            for pixel in row_pixels {
                for &sample in pixel {
                    out.extend_from_slice(&sample.to_be_bytes());
                }
            }
        }
    }
}

fn target_alpha(target: &EncodingTarget) -> impl Iterator<Item = u8> + '_ {
    let trns = target.trns.as_deref().unwrap_or(&[]);
    (0..target.palette.as_ref().map_or(0, Vec::len))
        .map(move |index| trns.get(index).copied().unwrap_or(255))
}

fn pack_samples_to(out: &mut Vec<u8>, samples: &[u8], bit_depth: u8) {
    if bit_depth == 8 {
        out.extend_from_slice(samples);
        return;
    }
    let mut acc = 0u16;
    let mut bits = 0usize;
    for &sample in samples {
        acc = (acc << bit_depth) | u16::from(sample);
        bits += usize::from(bit_depth);
        if bits >= 8 {
            out.push((acc >> (bits - 8)) as u8);
            bits -= 8;
            acc &= (1u16 << bits).saturating_sub(1);
        }
    }
    if bits > 0 {
        out.push((acc << (8 - bits)) as u8);
    }
}

fn quantize_grayscale_sample(sample: u8, bit_depth: u8) -> u8 {
    match bit_depth {
        1 => sample / 255,
        2 => sample / 85,
        4 => sample / 17,
        8 => sample,
        _ => unreachable!(),
    }
}

fn packed_stride_for_width(header: &PngHeader, width: u32) -> Result<usize> {
    (width as usize)
        .checked_mul(header.bits_per_pixel())
        .map(|bits| bits.div_ceil(8))
        .ok_or_else(|| Error::InvalidData("scanline stride overflow".into()))
}

fn decode_adam7_to_pixels(
    header: &PngHeader,
    filtered: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<PngPixels<'static>> {
    let pixel_count = (header.width as usize)
        .checked_mul(header.height as usize)
        .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
    let mut offset = 0usize;

    match (
        header.color_type,
        header.bit_depth,
        ancillary.transparency.is_some(),
    ) {
        (0, 1, false) | (0, 2, false) | (0, 4, false) => {
            let mut samples = vec![0u8; pixel_count];
            for pass in ADAM7_PASSES {
                let (pass_width, pass_height, pass_filtered) =
                    adam7_pass_window(header, filtered, &mut offset, pass)?;
                let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
                let pass_pixels = convert_grayscale_low_bit_to_pixels(
                    header,
                    pass_width,
                    &pass_raw,
                    ancillary,
                    header.bit_depth,
                )?;
                let pass_samples = match pass_pixels {
                    PngPixels::Gray1(data) | PngPixels::Gray2(data) | PngPixels::Gray4(data) => {
                        data.into_owned()
                    }
                    _ => unreachable!(),
                };
                scatter_scalar_u8(
                    &mut samples,
                    header.width,
                    pass,
                    pass_width,
                    pass_height,
                    &pass_samples,
                );
            }
            finish_adam7_offset(filtered, offset)?;
            Ok(match header.bit_depth {
                1 => PngPixels::Gray1(Cow::Owned(samples)),
                2 => PngPixels::Gray2(Cow::Owned(samples)),
                4 => PngPixels::Gray4(Cow::Owned(samples)),
                _ => unreachable!(),
            })
        }
        (3, 1 | 2 | 4 | 8, _) => {
            let mut indices = vec![0u8; pixel_count];
            for pass in ADAM7_PASSES {
                let (pass_width, pass_height, pass_filtered) =
                    adam7_pass_window(header, filtered, &mut offset, pass)?;
                let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
                let pass_pixels =
                    convert_indexed_to_pixels(header, pass_width, &pass_raw, ancillary)?;
                let pass_indices = match pass_pixels {
                    PngPixels::Indexed1 { indices, .. }
                    | PngPixels::Indexed2 { indices, .. }
                    | PngPixels::Indexed4 { indices, .. }
                    | PngPixels::Indexed8 { indices, .. } => indices.into_owned(),
                    _ => unreachable!(),
                };
                scatter_scalar_u8(
                    &mut indices,
                    header.width,
                    pass,
                    pass_width,
                    pass_height,
                    &pass_indices,
                );
            }
            finish_adam7_offset(filtered, offset)?;
            let palette = ancillary
                .palette
                .as_ref()
                .ok_or_else(|| Error::InvalidData("missing PLTE for palette image".into()))?
                .clone();
            let flat_palette = flatten_palette(&palette);
            let trns = match ancillary.transparency.as_ref() {
                Some(Transparency::Palette(alpha)) => Some(Cow::Owned(alpha.clone())),
                _ => None,
            };
            Ok(match header.bit_depth {
                1 => PngPixels::Indexed1 {
                    indices: Cow::Owned(indices),
                    palette: Cow::Owned(flat_palette.clone()),
                    trns,
                },
                2 => PngPixels::Indexed2 {
                    indices: Cow::Owned(indices),
                    palette: Cow::Owned(flat_palette.clone()),
                    trns,
                },
                4 => PngPixels::Indexed4 {
                    indices: Cow::Owned(indices),
                    palette: Cow::Owned(flat_palette.clone()),
                    trns,
                },
                8 => PngPixels::Indexed8 {
                    indices: Cow::Owned(indices),
                    palette: Cow::Owned(flat_palette),
                    trns,
                },
                _ => unreachable!(),
            })
        }
        _ => {
            let mut rgba = vec![0; expected_rgba16_len(header)?];
            for pass in ADAM7_PASSES {
                let (pass_width, pass_height, pass_filtered) =
                    adam7_pass_window(header, filtered, &mut offset, pass)?;
                let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
                let pass_rgba =
                    convert_to_rgba16(header, pass_width, pass_height, &pass_raw, ancillary)?;
                scatter_adam7_pass16(
                    &mut rgba,
                    header.width,
                    pass,
                    pass_width,
                    pass_height,
                    &pass_rgba,
                );
            }
            finish_adam7_offset(filtered, offset)?;
            Ok(pixels_from_rgba16_source(header, ancillary, &rgba))
        }
    }
}

fn adam7_pass_window<'a>(
    header: &PngHeader,
    filtered: &'a [u8],
    offset: &mut usize,
    pass: Adam7Pass,
) -> Result<(u32, u32, &'a [u8])> {
    let pass_width = adam7_axis_size(header.width, pass.x_start, pass.x_step);
    let pass_height = adam7_axis_size(header.height, pass.y_start, pass.y_step);
    if pass_width == 0 || pass_height == 0 {
        return Ok((0, 0, &[]));
    }
    let pass_stride = packed_stride_for_width(header, pass_width)?;
    let pass_filtered_len = (pass_stride + 1)
        .checked_mul(pass_height as usize)
        .ok_or_else(|| Error::InvalidData("Adam7 pass size overflow".into()))?;
    let pass_filtered = filtered
        .get(*offset..*offset + pass_filtered_len)
        .ok_or_else(|| Error::InvalidData("truncated Adam7 data".into()))?;
    *offset += pass_filtered_len;
    Ok((pass_width, pass_height, pass_filtered))
}

fn finish_adam7_offset(filtered: &[u8], offset: usize) -> Result<()> {
    if offset != filtered.len() {
        Err(Error::InvalidData(
            format!(
                "unexpected Adam7 data size: consumed {}, got {}",
                offset,
                filtered.len()
            )
            .into(),
        ))
    } else {
        Ok(())
    }
}

fn scatter_scalar_u8(
    full: &mut [u8],
    full_width: u32,
    pass: Adam7Pass,
    pass_width: u32,
    pass_height: u32,
    pass_values: &[u8],
) {
    for pass_y in 0..pass_height as usize {
        for pass_x in 0..pass_width as usize {
            let x = pass.x_start as usize + pass_x * pass.x_step as usize;
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            full[y * full_width as usize + x] = pass_values[pass_y * pass_width as usize + pass_x];
        }
    }
}

fn scatter_adam7_pass16(
    full_rgba: &mut [u16],
    full_width: u32,
    pass: Adam7Pass,
    pass_width: u32,
    pass_height: u32,
    pass_rgba: &[u16],
) {
    for pass_y in 0..pass_height as usize {
        for pass_x in 0..pass_width as usize {
            let x = pass.x_start as usize + pass_x * pass.x_step as usize;
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            let dst = (y * full_width as usize + x) * 4;
            let src = (pass_y * pass_width as usize + pass_x) * 4;
            full_rgba[dst..dst + 4].copy_from_slice(&pass_rgba[src..src + 4]);
        }
    }
}

fn adam7_axis_size(size: u32, start: u8, step: u8) -> u32 {
    if size <= u32::from(start) {
        0
    } else {
        (size - u32::from(start)).div_ceil(u32::from(step))
    }
}

#[derive(Debug, Clone, Copy)]
struct Adam7Pass {
    x_start: u8,
    y_start: u8,
    x_step: u8,
    y_step: u8,
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let a = i32::from(a);
    let b = i32::from(b);
    let c = i32::from(c);
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a as u8
    } else if pb <= pc {
        b as u8
    } else {
        c as u8
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn read_u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.read_array::<4>()?))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self.read_bytes(N)?;
        Ok(bytes
            .try_into()
            .expect("bug: read_array must return exactly N bytes"))
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| Error::InvalidData("PNG chunk size overflow".into()))?;
        let Some(bytes) = self.bytes.get(self.offset..end) else {
            return Err(Error::InvalidData("unexpected end of PNG stream".into()));
        };
        self.offset = end;
        Ok(bytes)
    }
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
