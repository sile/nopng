use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::chunk::IhdrChunk;
use crate::pixel_reformat::{scale_sample_to_u8, upscale_u8_to_u16};
use crate::png_types::{Error, PixelFormat, Result};
use crate::{adler32, crc, deflate};

use crate::png::{ADAM7_PASSES, Adam7Pass, PNG_SIGNATURE, adam7_axis_size};

// Short aliases for color type constants used in match patterns.
const CT_GRAY: u8 = IhdrChunk::COLOR_TYPE_GRAYSCALE;
const CT_RGB: u8 = IhdrChunk::COLOR_TYPE_RGB;
const CT_INDEXED: u8 = IhdrChunk::COLOR_TYPE_INDEXED;
const CT_GRAY_ALPHA: u8 = IhdrChunk::COLOR_TYPE_GRAYSCALE_ALPHA;
const CT_RGBA: u8 = IhdrChunk::COLOR_TYPE_RGBA;

#[derive(Debug, Clone, Copy)]
pub(crate) struct PngHeader {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bit_depth: u8,
    pub(crate) color_type: u8,
    pub(crate) compression_method: u8,
    pub(crate) filter_method: u8,
    pub(crate) interlace_method: u8,
}

impl PngHeader {
    pub(crate) fn parse(chunk_data: &[u8]) -> Result<Self> {
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

    pub(crate) fn validate(&self) -> Result<()> {
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
            (CT_GRAY, 1 | 2 | 4 | 8 | 16)
            | (CT_INDEXED, 1 | 2 | 4 | 8)
            | (CT_RGB, 8 | 16)
            | (CT_GRAY_ALPHA, 8 | 16)
            | (CT_RGBA, 8 | 16) => Ok(()),
            _ => Err(Error::Unsupported(
                format!(
                    "unsupported color type/bit depth combination: color_type={}, bit_depth={}",
                    self.color_type, self.bit_depth
                )
                .into(),
            )),
        }
    }

    pub(crate) fn samples_per_pixel(&self) -> usize {
        match self.color_type {
            CT_GRAY | CT_INDEXED => 1,
            CT_RGB => 3,
            CT_GRAY_ALPHA => 2,
            CT_RGBA => 4,
            _ => unreachable!(),
        }
    }

    pub(crate) fn bits_per_pixel(&self) -> usize {
        self.samples_per_pixel() * usize::from(self.bit_depth)
    }

    pub(crate) fn bytes_per_pixel(&self) -> usize {
        self.bits_per_pixel().div_ceil(8)
    }

    pub(crate) fn filter_bpp(&self) -> usize {
        if self.bit_depth < 8 {
            1
        } else {
            self.bytes_per_pixel()
        }
    }
}

#[derive(Debug, Clone)]
enum Transparency {
    Grayscale(u16),
    Truecolor([u16; 3]),
    Palette(Vec<u8>),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AncillaryChunks {
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
        if header.color_type == CT_INDEXED && self.palette.is_none() {
            return Err(Error::InvalidData("missing PLTE for palette image".into()));
        }
        if matches!(header.color_type, CT_GRAY | CT_GRAY_ALPHA) && self.palette.is_some() {
            return Err(Error::InvalidData(
                "PLTE chunk is not allowed for grayscale images".into(),
            ));
        }
        match (&self.transparency, header.color_type) {
            (Some(Transparency::Grayscale(_)), CT_GRAY) => {}
            (Some(Transparency::Truecolor(_)), CT_RGB) => {}
            (Some(Transparency::Palette(alpha)), CT_INDEXED) => {
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

    /// Flat RGB palette bytes.
    pub(crate) fn flat_palette(&self) -> Option<Vec<u8>> {
        self.palette.as_ref().map(|p| flatten_palette(p))
    }

    /// tRNS bytes for indexed images.
    pub(crate) fn indexed_trns(&self) -> Option<Vec<u8>> {
        match &self.transparency {
            Some(Transparency::Palette(alpha)) => Some(alpha.clone()),
            _ => None,
        }
    }

    /// Returns true if any tRNS transparency is present.
    pub(crate) fn has_transparency(&self) -> bool {
        self.transparency.is_some()
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

/// Decodes a PNG byte stream into a header and pixel data.
pub(crate) fn decode_png(
    bytes: &[u8],
) -> Result<(PngHeader, AncillaryChunks, PixelFormat, Vec<u8>)> {
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
    let (format, data) = decode_to_format_and_data(&header, &filtered, &ancillary)?;
    Ok((header, ancillary, format, data))
}

/// Parses PNG header and metadata chunks (IHDR, PLTE, tRNS), stopping at IDAT.
pub(crate) fn parse_png_metadata(bytes: &[u8]) -> Result<(PngHeader, AncillaryChunks)> {
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(Error::InvalidData("invalid PNG signature".into()));
    }

    let mut cursor = Cursor::new(&bytes[PNG_SIGNATURE.len()..]);
    let mut header = None;
    let mut ancillary = AncillaryChunks::default();

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
                header = Some(PngHeader::parse(chunk_data)?);
            }
            b"PLTE" => {
                let Some(header) = header else {
                    return Err(Error::InvalidData("PLTE chunk before IHDR".into()));
                };
                if matches!(header.color_type, CT_GRAY | CT_GRAY_ALPHA) {
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
                ancillary.set_transparency(parse_transparency(chunk_data, &header, &ancillary)?)?;
            }
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
    }

    let header = header.ok_or_else(|| Error::InvalidData("missing IHDR chunk".into()))?;
    Ok((header, ancillary))
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
                if matches!(header.color_type, CT_GRAY | CT_GRAY_ALPHA) {
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
        CT_GRAY => {
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
        CT_RGB => {
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
        CT_INDEXED => {
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

fn decode_to_format_and_data(
    header: &PngHeader,
    filtered: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<(PixelFormat, Vec<u8>)> {
    if header.interlace_method == 0 {
        let raw = unfilter_scanlines(header, header.width, header.height, filtered)?;
        convert_to_format_and_data(header, header.width, &raw, ancillary)
    } else {
        decode_adam7_to_format_and_data(header, filtered, ancillary)
    }
}

fn convert_to_format_and_data(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<(PixelFormat, Vec<u8>)> {
    match (header.color_type, header.bit_depth) {
        (CT_GRAY, 1 | 2 | 4) => convert_grayscale_low_bit(header, width, raw, ancillary),
        (CT_GRAY, 8) => {
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
                Ok((PixelFormat::GrayAlpha8, data))
            } else {
                Ok((PixelFormat::Gray8, raw.to_vec()))
            }
        }
        (CT_GRAY, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            if let Some(transparent) = transparent {
                // Output GrayAlpha16Be: [g_hi, g_lo, a_hi, a_lo, ...]
                let (samples, remainder) = raw.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                let mut data = Vec::with_capacity(samples.len() * 4);
                for chunk in samples {
                    let gray = u16::from_be_bytes(*chunk);
                    let alpha: u16 = if gray == transparent { 0 } else { u16::MAX };
                    data.extend_from_slice(chunk);
                    data.extend_from_slice(&alpha.to_be_bytes());
                }
                Ok((PixelFormat::GrayAlpha16Be, data))
            } else {
                // Pass raw BE bytes through directly.
                Ok((PixelFormat::Gray16Be, raw.to_vec()))
            }
        }
        (CT_RGB, 8) => {
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
                Ok((PixelFormat::Rgba8, data))
            } else {
                Ok((PixelFormat::Rgb8, raw.to_vec()))
            }
        }
        (CT_RGB, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            if let Some(transparent) = transparent {
                // Output Rgba16Be: [r_hi, r_lo, g_hi, g_lo, b_hi, b_lo, a_hi, a_lo, ...]
                let (pixels, remainder) = raw.as_chunks::<6>();
                debug_assert!(remainder.is_empty());
                let mut data = Vec::with_capacity(pixels.len() * 8);
                for &[r0, r1, g0, g1, b0, b1] in pixels {
                    let rgb = [
                        u16::from_be_bytes([r0, r1]),
                        u16::from_be_bytes([g0, g1]),
                        u16::from_be_bytes([b0, b1]),
                    ];
                    let alpha: u16 = if rgb == transparent { 0 } else { u16::MAX };
                    data.extend_from_slice(&[r0, r1, g0, g1, b0, b1]);
                    data.extend_from_slice(&alpha.to_be_bytes());
                }
                Ok((PixelFormat::Rgba16Be, data))
            } else {
                // Pass raw BE bytes through directly.
                Ok((PixelFormat::Rgb16Be, raw.to_vec()))
            }
        }
        (CT_INDEXED, 1 | 2 | 4 | 8) => convert_indexed(header, width, raw, ancillary),
        (CT_GRAY_ALPHA, 8) => Ok((PixelFormat::GrayAlpha8, raw.to_vec())),
        (CT_GRAY_ALPHA, 16) => {
            // Raw bytes are already BE: [g_hi, g_lo, a_hi, a_lo, ...]
            Ok((PixelFormat::GrayAlpha16Be, raw.to_vec()))
        }
        (CT_RGBA, 8) => Ok((PixelFormat::Rgba8, raw.to_vec())),
        (CT_RGBA, 16) => {
            // Raw bytes are already BE: [r_hi, r_lo, g_hi, g_lo, b_hi, b_lo, a_hi, a_lo, ...]
            Ok((PixelFormat::Rgba16Be, raw.to_vec()))
        }
        _ => unreachable!(),
    }
}

fn convert_grayscale_low_bit(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<(PixelFormat, Vec<u8>)> {
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
            let gray = scale_sample_to_u8(u16::from(sample), header.bit_depth);
            let alpha = if u16::from(sample) == transparent {
                0
            } else {
                255
            };
            data.extend_from_slice(&[gray, alpha]);
        }
        Ok((PixelFormat::GrayAlpha8, data))
    } else {
        Ok((gray_format_from_bit_depth(header.bit_depth), unpacked))
    }
}

fn convert_indexed(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<(PixelFormat, Vec<u8>)> {
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
    let flat_palette = flatten_palette(palette);
    let format = indexed_format_from_bit_depth(header.bit_depth, flat_palette, trns);
    Ok((format, unpacked))
}

/// Convert raw data into RGBA16Be intermediate for Adam7 scatter (fallback path).
fn convert_to_rgba16be(
    header: &PngHeader,
    width: u32,
    height: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<Vec<u8>> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
    let mut rgba = Vec::with_capacity(pixel_count * 8);
    match (header.color_type, header.bit_depth) {
        (CT_GRAY, 1 | 2 | 4) => {
            let row_stride = packed_stride_for_width(header, width)?;
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            for row in raw.chunks_exact(row_stride) {
                for sample in unpack_samples(row, width as usize, header.bit_depth) {
                    let gray =
                        upscale_u8_to_u16(scale_sample_to_u8(u16::from(sample), header.bit_depth));
                    let alpha: u16 = if Some(u16::from(sample)) == transparent {
                        0
                    } else {
                        u16::MAX
                    };
                    let gb = gray.to_be_bytes();
                    rgba.extend_from_slice(&[gb[0], gb[1], gb[0], gb[1], gb[0], gb[1]]);
                    rgba.extend_from_slice(&alpha.to_be_bytes());
                }
            }
        }
        (CT_GRAY, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            for &gray in raw {
                let gray16 = upscale_u8_to_u16(gray);
                let alpha: u16 = if Some(u16::from(crate::pixel_reformat::downsample_u16(gray16)))
                    == transparent
                {
                    0
                } else {
                    u16::MAX
                };
                let gb = gray16.to_be_bytes();
                rgba.extend_from_slice(&[gb[0], gb[1], gb[0], gb[1], gb[0], gb[1]]);
                rgba.extend_from_slice(&alpha.to_be_bytes());
            }
        }
        (CT_GRAY, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            let (pixels, _) = raw.as_chunks::<2>();
            for chunk in pixels {
                let gray = u16::from_be_bytes(*chunk);
                let alpha: u16 = if Some(gray) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(chunk); // r
                rgba.extend_from_slice(chunk); // g
                rgba.extend_from_slice(chunk); // b
                rgba.extend_from_slice(&alpha.to_be_bytes());
            }
        }
        (CT_RGB, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            let (pixels, _) = raw.as_chunks::<3>();
            for &[r, g, b] in pixels {
                let rgb = [u16::from(r), u16::from(g), u16::from(b)];
                let alpha: u16 = if Some(rgb) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(&upscale_u8_to_u16(r).to_be_bytes());
                rgba.extend_from_slice(&upscale_u8_to_u16(g).to_be_bytes());
                rgba.extend_from_slice(&upscale_u8_to_u16(b).to_be_bytes());
                rgba.extend_from_slice(&alpha.to_be_bytes());
            }
        }
        (CT_RGB, 16) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            let (pixels, _) = raw.as_chunks::<6>();
            for &[r0, r1, g0, g1, b0, b1] in pixels {
                let rgb = [
                    u16::from_be_bytes([r0, r1]),
                    u16::from_be_bytes([g0, g1]),
                    u16::from_be_bytes([b0, b1]),
                ];
                let alpha: u16 = if Some(rgb) == transparent {
                    0
                } else {
                    u16::MAX
                };
                rgba.extend_from_slice(&[r0, r1, g0, g1, b0, b1]);
                rgba.extend_from_slice(&alpha.to_be_bytes());
            }
        }
        (CT_INDEXED, 1 | 2 | 4 | 8) => {
            let (fmt, indices) = convert_indexed(header, width, raw, ancillary)?;
            let expanded = crate::pixel_reformat::reformat(&fmt, &indices, &PixelFormat::Rgba16Be)?;
            rgba.extend(expanded);
        }
        (CT_GRAY_ALPHA, 8) => {
            let (pixels, _) = raw.as_chunks::<2>();
            for &[gray, alpha] in pixels {
                let g16 = upscale_u8_to_u16(gray);
                let a16 = upscale_u8_to_u16(alpha);
                let gb = g16.to_be_bytes();
                rgba.extend_from_slice(&[gb[0], gb[1], gb[0], gb[1], gb[0], gb[1]]);
                rgba.extend_from_slice(&a16.to_be_bytes());
            }
        }
        (CT_GRAY_ALPHA, 16) => {
            let (pixels, _) = raw.as_chunks::<4>();
            for &[g0, g1, a0, a1] in pixels {
                rgba.extend_from_slice(&[g0, g1, g0, g1, g0, g1, a0, a1]);
            }
        }
        (CT_RGBA, 8) => {
            let (pixels, _) = raw.as_chunks::<4>();
            for &[r, g, b, a] in pixels {
                rgba.extend_from_slice(&upscale_u8_to_u16(r).to_be_bytes());
                rgba.extend_from_slice(&upscale_u8_to_u16(g).to_be_bytes());
                rgba.extend_from_slice(&upscale_u8_to_u16(b).to_be_bytes());
                rgba.extend_from_slice(&upscale_u8_to_u16(a).to_be_bytes());
            }
        }
        (CT_RGBA, 16) => {
            // Already in RGBA16Be format.
            rgba.extend_from_slice(raw);
        }
        _ => unreachable!(),
    }
    Ok(rgba)
}

fn expected_rgba16be_len(header: &PngHeader) -> Result<usize> {
    (header.width as usize)
        .checked_mul(header.height as usize)
        .and_then(|pixels| pixels.checked_mul(8))
        .ok_or_else(|| Error::InvalidData("decoded RGBA16Be size overflow".into()))
}

/// Convert scattered RGBA16Be back to source-near format.
fn format_from_rgba16be_source(
    header: &PngHeader,
    ancillary: &AncillaryChunks,
    rgba: &[u8],
) -> (PixelFormat, Vec<u8>) {
    match (
        header.color_type,
        header.bit_depth,
        ancillary.transparency.is_some(),
    ) {
        (CT_GRAY, 16, false) => {
            // Extract gray channel (first 2 bytes of each 8-byte chunk).
            let (chunks, _) = rgba.as_chunks::<8>();
            let mut data = Vec::with_capacity(chunks.len() * 2);
            for chunk in chunks {
                data.extend_from_slice(&chunk[..2]);
            }
            (PixelFormat::Gray16Be, data)
        }
        (CT_GRAY, _, true) => {
            if header.bit_depth == 16 {
                // GrayAlpha16Be
                let (chunks, _) = rgba.as_chunks::<8>();
                let mut data = Vec::with_capacity(chunks.len() * 4);
                for chunk in chunks {
                    data.extend_from_slice(&chunk[..2]); // gray
                    data.extend_from_slice(&chunk[6..8]); // alpha
                }
                (PixelFormat::GrayAlpha16Be, data)
            } else {
                // GrayAlpha8
                let (chunks, _) = rgba.as_chunks::<8>();
                let mut data = Vec::with_capacity(chunks.len() * 2);
                for chunk in chunks {
                    data.push(chunk[0]); // gray high byte
                    data.push(chunk[6]); // alpha high byte
                }
                (PixelFormat::GrayAlpha8, data)
            }
        }
        (CT_RGB, 16, false) => {
            // Rgb16Be: extract first 6 bytes of each 8-byte chunk.
            let (chunks, _) = rgba.as_chunks::<8>();
            let mut data = Vec::with_capacity(chunks.len() * 6);
            for chunk in chunks {
                data.extend_from_slice(&chunk[..6]);
            }
            (PixelFormat::Rgb16Be, data)
        }
        (CT_RGB, _, true) => {
            if header.bit_depth == 16 {
                (PixelFormat::Rgba16Be, rgba.to_vec())
            } else {
                // Downsample to Rgba8
                let (chunks, _) = rgba.as_chunks::<8>();
                let mut data = Vec::with_capacity(chunks.len() * 4);
                for chunk in chunks {
                    data.extend_from_slice(&[chunk[0], chunk[2], chunk[4], chunk[6]]);
                }
                (PixelFormat::Rgba8, data)
            }
        }
        (CT_GRAY_ALPHA, 8, _) => {
            // Downsample to GrayAlpha8
            let (chunks, _) = rgba.as_chunks::<8>();
            let mut data = Vec::with_capacity(chunks.len() * 2);
            for chunk in chunks {
                data.push(chunk[0]); // gray high byte
                data.push(chunk[6]); // alpha high byte
            }
            (PixelFormat::GrayAlpha8, data)
        }
        (CT_GRAY_ALPHA, 16, _) => {
            let (chunks, _) = rgba.as_chunks::<8>();
            let mut data = Vec::with_capacity(chunks.len() * 4);
            for chunk in chunks {
                data.extend_from_slice(&chunk[..2]); // gray
                data.extend_from_slice(&chunk[6..8]); // alpha
            }
            (PixelFormat::GrayAlpha16Be, data)
        }
        (CT_RGBA, 8, _) => {
            // Downsample to Rgba8
            let (chunks, _) = rgba.as_chunks::<8>();
            let mut data = Vec::with_capacity(chunks.len() * 4);
            for chunk in chunks {
                data.extend_from_slice(&[chunk[0], chunk[2], chunk[4], chunk[6]]);
            }
            (PixelFormat::Rgba8, data)
        }
        (CT_RGBA, 16, _) => (PixelFormat::Rgba16Be, rgba.to_vec()),
        _ => {
            // Low-bit grayscale (no tRNS): downsample to original bit depth.
            let (chunks, _) = rgba.as_chunks::<8>();
            let data: Vec<u8> = chunks.iter().map(|chunk| chunk[0]).collect();
            let samples = match header.bit_depth {
                1 => data.iter().copied().map(|v| v / 255).collect(),
                2 => data.iter().copied().map(|v| v / 85).collect(),
                4 => data.iter().copied().map(|v| v / 17).collect(),
                8 => data,
                _ => unreachable!(),
            };
            (gray_format_from_bit_depth(header.bit_depth), samples)
        }
    }
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

fn unpack_samples(bytes: &[u8], width: usize, bit_depth: u8) -> impl Iterator<Item = u8> + '_ {
    let mask = (1u16 << bit_depth) - 1;
    (0..width).map(move |pixel| {
        let bit_offset = pixel * usize::from(bit_depth);
        let byte = bytes[bit_offset / 8];
        let shift = 8 - usize::from(bit_depth) - (bit_offset % 8);
        ((u16::from(byte) >> shift) & mask) as u8
    })
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

pub(crate) fn expected_filtered_len(header: &PngHeader) -> Result<usize> {
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

pub(crate) fn packed_stride_for_width(header: &PngHeader, width: u32) -> Result<usize> {
    (width as usize)
        .checked_mul(header.bits_per_pixel())
        .map(|bits| bits.div_ceil(8))
        .ok_or_else(|| Error::InvalidData("scanline stride overflow".into()))
}

fn decode_adam7_to_format_and_data(
    header: &PngHeader,
    filtered: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<(PixelFormat, Vec<u8>)> {
    let pixel_count = (header.width as usize)
        .checked_mul(header.height as usize)
        .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
    let mut offset = 0usize;

    match (
        header.color_type,
        header.bit_depth,
        ancillary.transparency.is_some(),
    ) {
        (CT_GRAY, 1, false) | (CT_GRAY, 2, false) | (CT_GRAY, 4, false) => {
            let mut samples = vec![0u8; pixel_count];
            for pass in ADAM7_PASSES {
                let (pass_width, pass_height, pass_filtered) =
                    adam7_pass_window(header, filtered, &mut offset, pass)?;
                let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
                let (_, pass_data) =
                    convert_grayscale_low_bit(header, pass_width, &pass_raw, ancillary)?;
                scatter_scalar_u8(
                    &mut samples,
                    header.width,
                    pass,
                    pass_width,
                    pass_height,
                    &pass_data,
                );
            }
            finish_adam7_offset(filtered, offset)?;
            Ok((gray_format_from_bit_depth(header.bit_depth), samples))
        }
        (CT_INDEXED, 1 | 2 | 4 | 8, _) => {
            let mut indices = vec![0u8; pixel_count];
            for pass in ADAM7_PASSES {
                let (pass_width, pass_height, pass_filtered) =
                    adam7_pass_window(header, filtered, &mut offset, pass)?;
                let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
                let (_, pass_data) = convert_indexed(header, pass_width, &pass_raw, ancillary)?;
                scatter_scalar_u8(
                    &mut indices,
                    header.width,
                    pass,
                    pass_width,
                    pass_height,
                    &pass_data,
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
                Some(Transparency::Palette(alpha)) => Some(alpha.clone()),
                _ => None,
            };
            let format = indexed_format_from_bit_depth(header.bit_depth, flat_palette, trns);
            Ok((format, indices))
        }
        // 8-bit types without tRNS: scatter raw bytes directly.
        (CT_GRAY, 8, false) => {
            let data = adam7_scatter_raw_bytes(header, filtered, &mut offset, 1)?;
            finish_adam7_offset(filtered, offset)?;
            Ok((PixelFormat::Gray8, data))
        }
        (CT_GRAY_ALPHA, 8, _) => {
            let data = adam7_scatter_raw_bytes(header, filtered, &mut offset, 2)?;
            finish_adam7_offset(filtered, offset)?;
            Ok((PixelFormat::GrayAlpha8, data))
        }
        (CT_RGB, 8, false) => {
            let data = adam7_scatter_raw_bytes(header, filtered, &mut offset, 3)?;
            finish_adam7_offset(filtered, offset)?;
            Ok((PixelFormat::Rgb8, data))
        }
        (CT_RGBA, 8, _) => {
            let data = adam7_scatter_raw_bytes(header, filtered, &mut offset, 4)?;
            finish_adam7_offset(filtered, offset)?;
            Ok((PixelFormat::Rgba8, data))
        }
        _ => {
            // Fallback: use RGBA16Be intermediate and scatter.
            let mut rgba = vec![0u8; expected_rgba16be_len(header)?];
            for pass in ADAM7_PASSES {
                let (pass_width, pass_height, pass_filtered) =
                    adam7_pass_window(header, filtered, &mut offset, pass)?;
                let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
                let pass_rgba =
                    convert_to_rgba16be(header, pass_width, pass_height, &pass_raw, ancillary)?;
                scatter_bytes(
                    &mut rgba,
                    header.width,
                    pass,
                    pass_width,
                    pass_height,
                    &pass_rgba,
                    8, // RGBA16Be = 8 bytes per pixel
                );
            }
            finish_adam7_offset(filtered, offset)?;
            Ok(format_from_rgba16be_source(header, ancillary, &rgba))
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

fn adam7_scatter_raw_bytes(
    header: &PngHeader,
    filtered: &[u8],
    offset: &mut usize,
    bytes_per_pixel: usize,
) -> Result<Vec<u8>> {
    let pixel_count = (header.width as usize)
        .checked_mul(header.height as usize)
        .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
    let mut data = vec![0u8; pixel_count * bytes_per_pixel];
    for pass in ADAM7_PASSES {
        let (pass_width, pass_height, pass_filtered) =
            adam7_pass_window(header, filtered, offset, pass)?;
        let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
        scatter_bytes(
            &mut data,
            header.width,
            pass,
            pass_width,
            pass_height,
            &pass_raw,
            bytes_per_pixel,
        );
    }
    Ok(data)
}

fn scatter_bytes(
    full: &mut [u8],
    full_width: u32,
    pass: Adam7Pass,
    pass_width: u32,
    pass_height: u32,
    pass_data: &[u8],
    bytes_per_pixel: usize,
) {
    for pass_y in 0..pass_height as usize {
        for pass_x in 0..pass_width as usize {
            let x = pass.x_start as usize + pass_x * pass.x_step as usize;
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            let dst = (y * full_width as usize + x) * bytes_per_pixel;
            let src = (pass_y * pass_width as usize + pass_x) * bytes_per_pixel;
            full[dst..dst + bytes_per_pixel]
                .copy_from_slice(&pass_data[src..src + bytes_per_pixel]);
        }
    }
}

fn gray_format_from_bit_depth(bit_depth: u8) -> PixelFormat {
    match bit_depth {
        1 => PixelFormat::Gray1,
        2 => PixelFormat::Gray2,
        4 => PixelFormat::Gray4,
        8 => PixelFormat::Gray8,
        16 => PixelFormat::Gray16Be,
        _ => unreachable!("bug: validated grayscale bit depth must be 1/2/4/8/16"),
    }
}

fn indexed_format_from_bit_depth(
    bit_depth: u8,
    palette: Vec<u8>,
    trns: Option<Vec<u8>>,
) -> PixelFormat {
    match bit_depth {
        1 => PixelFormat::Indexed1 { palette, trns },
        2 => PixelFormat::Indexed2 { palette, trns },
        4 => PixelFormat::Indexed4 { palette, trns },
        8 => PixelFormat::Indexed8 { palette, trns },
        _ => unreachable!("bug: validated indexed bit depth must be 1/2/4/8"),
    }
}

fn flatten_palette(palette: &[[u8; 3]]) -> Vec<u8> {
    let mut flattened = Vec::with_capacity(palette.len() * 3);
    for &[r, g, b] in palette {
        flattened.extend_from_slice(&[r, g, b]);
    }
    flattened
}
