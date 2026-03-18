use std::io::{Read, Write};

use crate::chunk::{IdatChunk, IendChunk, IhdrChunk, PlteChunk, TrnsChunk};
use crate::{adler32, crc, deflate};

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

#[derive(Debug)]
pub enum PngDecodeError {
    Io(std::io::Error),
    InvalidSignature,
    InvalidChunk(String),
    Unsupported(String),
    InvalidData(String),
}

#[derive(Debug)]
pub enum PngEncodeError {
    Io(std::io::Error),
    Unsupported(String),
    InvalidData(String),
}

impl std::fmt::Display for PngEncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Unsupported(message) => f.write_str(message),
            Self::InvalidData(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PngEncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Unsupported(_) | Self::InvalidData(_) => None,
        }
    }
}

impl From<std::io::Error> for PngEncodeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngColorMode {
    Auto,
    Grayscale,
    GrayscaleAlpha,
    Rgb,
    Rgba,
    Indexed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngEncodeOptions {
    pub color_mode: PngColorMode,
    pub interlaced: bool,
}

impl Default for PngEncodeOptions {
    fn default() -> Self {
        Self {
            color_mode: PngColorMode::Auto,
            interlaced: false,
        }
    }
}

impl std::fmt::Display for PngDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::InvalidSignature => f.write_str("invalid PNG signature"),
            Self::InvalidChunk(message) => f.write_str(message),
            Self::Unsupported(message) => f.write_str(message),
            Self::InvalidData(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PngDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidSignature
            | Self::InvalidChunk(_)
            | Self::Unsupported(_)
            | Self::InvalidData(_) => None,
        }
    }
}

impl From<std::io::Error> for PngDecodeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct PngImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl PngImage {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        if (width * height * 4) as usize != data.len() {
            None
        } else {
            Some(Self {
                width,
                height,
                data,
            })
        }
    }

    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, PngDecodeError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PngDecodeError> {
        if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
            return Err(PngDecodeError::InvalidSignature);
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

            let mut crc_input = Vec::with_capacity(4 + chunk_data.len());
            crc_input.extend_from_slice(&chunk_type);
            crc_input.extend_from_slice(chunk_data);
            let actual_crc = crc::calculate(&crc_input);
            if actual_crc != expected_crc {
                return Err(PngDecodeError::InvalidChunk(format!(
                    "CRC mismatch for chunk {}",
                    std::str::from_utf8(&chunk_type).unwrap_or("????"),
                )));
            }

            match &chunk_type {
                b"IHDR" => {
                    if header.is_some() {
                        return Err(PngDecodeError::InvalidChunk("duplicate IHDR chunk".into()));
                    }
                    if seen_idat {
                        return Err(PngDecodeError::InvalidChunk("IHDR chunk after IDAT".into()));
                    }
                    header = Some(PngHeader::parse(chunk_data)?);
                }
                b"PLTE" => {
                    let Some(header) = header else {
                        return Err(PngDecodeError::InvalidChunk(
                            "PLTE chunk before IHDR".into(),
                        ));
                    };
                    if seen_idat {
                        return Err(PngDecodeError::InvalidChunk(
                            "PLTE appears after IDAT".into(),
                        ));
                    }
                    ancillary.set_palette(parse_palette(chunk_data, header.color_type)?)?;
                }
                b"tRNS" => {
                    let Some(header) = header else {
                        return Err(PngDecodeError::InvalidChunk(
                            "tRNS chunk before IHDR".into(),
                        ));
                    };
                    if seen_idat {
                        return Err(PngDecodeError::InvalidChunk(
                            "tRNS appears after IDAT".into(),
                        ));
                    }
                    ancillary
                        .set_transparency(parse_transparency(chunk_data, &header, &ancillary)?)?;
                }
                b"IDAT" => {
                    if header.is_none() {
                        return Err(PngDecodeError::InvalidChunk(
                            "IDAT chunk before IHDR".into(),
                        ));
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
            return Err(PngDecodeError::InvalidChunk("missing IEND chunk".into()));
        }
        let header =
            header.ok_or_else(|| PngDecodeError::InvalidChunk("missing IHDR chunk".into()))?;
        if idat_data.is_empty() {
            return Err(PngDecodeError::InvalidChunk("missing IDAT chunk".into()));
        }
        ancillary.validate(&header)?;

        let filtered = decompress_zlib(&idat_data)?;
        let rgba = decode_to_rgba(&header, &filtered, &ancillary)?;
        Self::new(header.width, header.height, rgba)
            .ok_or_else(|| PngDecodeError::InvalidData("decoded image size mismatch".into()))
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.write_to_with_options(writer, PngEncodeOptions::default())
            .map_err(encode_error_into_io)
    }

    pub fn write_to_with_options<W: Write>(
        &self,
        writer: &mut W,
        options: PngEncodeOptions,
    ) -> Result<(), PngEncodeError> {
        let encoded = EncodedImage::from_rgba(self.width, self.height, &self.data, options)?;
        writer.write_all(&PNG_SIGNATURE)?;

        IhdrChunk {
            width: self.width,
            height: self.height,
            bit_depth: encoded.bit_depth,
            color_type: encoded.color_type,
            interlace_method: encoded.interlace_method,
        }
        .write_to(writer)?;
        if let Some(palette) = encoded.palette.as_deref() {
            PlteChunk { palette }.write_to(writer)?;
        }
        if let Some(trns) = encoded.trns.as_deref() {
            TrnsChunk { data: trns }.write_to(writer)?;
        }
        IdatChunk {
            filtered_data: &encoded.filtered_data,
        }
        .write_to(writer)?;
        IendChunk.write_to(writer)?;

        Ok(())
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
    fn parse(chunk_data: &[u8]) -> Result<Self, PngDecodeError> {
        if chunk_data.len() != 13 {
            return Err(PngDecodeError::InvalidChunk(
                "IHDR chunk must contain 13 bytes".into(),
            ));
        }
        let width = u32::from_be_bytes(chunk_data[0..4].try_into().unwrap());
        let height = u32::from_be_bytes(chunk_data[4..8].try_into().unwrap());
        if width == 0 || height == 0 {
            return Err(PngDecodeError::InvalidData(
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

    fn validate(&self) -> Result<(), PngDecodeError> {
        if self.compression_method != 0 {
            return Err(PngDecodeError::Unsupported(format!(
                "unsupported compression method: {}",
                self.compression_method
            )));
        }
        if self.filter_method != 0 {
            return Err(PngDecodeError::Unsupported(format!(
                "unsupported filter method: {}",
                self.filter_method
            )));
        }
        if self.interlace_method > 1 {
            return Err(PngDecodeError::Unsupported(format!(
                "unsupported interlace method: {}",
                self.interlace_method
            )));
        }
        match (self.color_type, self.bit_depth) {
            (0, 1 | 2 | 4 | 8 | 16)
            | (3, 1 | 2 | 4 | 8)
            | (2, 8 | 16)
            | (4, 8 | 16)
            | (6, 8 | 16) => Ok(()),
            _ => Err(PngDecodeError::Unsupported(format!(
                "unsupported color type/bit depth combination: color_type={}, bit_depth={}",
                self.color_type, self.bit_depth
            ))),
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
    fn set_palette(&mut self, palette: Vec<[u8; 3]>) -> Result<(), PngDecodeError> {
        if self.palette.is_some() {
            return Err(PngDecodeError::InvalidChunk("duplicate PLTE chunk".into()));
        }
        self.palette = Some(palette);
        Ok(())
    }

    fn set_transparency(&mut self, transparency: Transparency) -> Result<(), PngDecodeError> {
        if self.transparency.is_some() {
            return Err(PngDecodeError::InvalidChunk("duplicate tRNS chunk".into()));
        }
        self.transparency = Some(transparency);
        Ok(())
    }

    fn validate(&self, header: &PngHeader) -> Result<(), PngDecodeError> {
        if header.color_type == 3 && self.palette.is_none() {
            return Err(PngDecodeError::InvalidChunk(
                "missing PLTE for palette image".into(),
            ));
        }
        if header.color_type != 3 && self.palette.is_some() {
            return Err(PngDecodeError::InvalidChunk(
                "PLTE chunk is only supported for palette images".into(),
            ));
        }
        match (&self.transparency, header.color_type) {
            (Some(Transparency::Grayscale(_)), 0) => {}
            (Some(Transparency::Truecolor(_)), 2) => {}
            (Some(Transparency::Palette(alpha)), 3) => {
                let palette_len = self.palette.as_ref().map_or(0, Vec::len);
                if alpha.len() > palette_len {
                    return Err(PngDecodeError::InvalidChunk(
                        "tRNS length exceeds palette length".into(),
                    ));
                }
            }
            (Some(_), _) => {
                return Err(PngDecodeError::InvalidChunk(format!(
                    "tRNS is not allowed for color type {}",
                    header.color_type
                )));
            }
            (None, _) => {}
        }
        Ok(())
    }
}

fn parse_palette(chunk_data: &[u8], color_type: u8) -> Result<Vec<[u8; 3]>, PngDecodeError> {
    if color_type != 3 {
        return Err(PngDecodeError::InvalidChunk(format!(
            "PLTE is not allowed for color type {}",
            color_type
        )));
    }
    if chunk_data.is_empty() || !chunk_data.len().is_multiple_of(3) {
        return Err(PngDecodeError::InvalidChunk(
            "PLTE length must be a non-zero multiple of 3".into(),
        ));
    }
    let palette = chunk_data
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect::<Vec<_>>();
    if palette.len() > 256 {
        return Err(PngDecodeError::InvalidChunk(
            "PLTE must not contain more than 256 entries".into(),
        ));
    }
    Ok(palette)
}

fn parse_transparency(
    chunk_data: &[u8],
    header: &PngHeader,
    ancillary: &AncillaryChunks,
) -> Result<Transparency, PngDecodeError> {
    match header.color_type {
        0 => {
            if chunk_data.len() != 2 {
                return Err(PngDecodeError::InvalidChunk(
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
                return Err(PngDecodeError::InvalidChunk(
                    "invalid grayscale transparency sample".into(),
                ));
            }
            Ok(Transparency::Grayscale(sample))
        }
        2 => {
            if chunk_data.len() != 6 {
                return Err(PngDecodeError::InvalidChunk(
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
                return Err(PngDecodeError::InvalidChunk(
                    "tRNS chunk must appear after PLTE".into(),
                ));
            }
            Ok(Transparency::Palette(chunk_data.to_vec()))
        }
        _ => Err(PngDecodeError::InvalidChunk(format!(
            "tRNS is not allowed for color type {}",
            header.color_type
        ))),
    }
}

fn decompress_zlib(data: &[u8]) -> Result<Vec<u8>, PngDecodeError> {
    if data.len() < 6 {
        return Err(PngDecodeError::InvalidData(
            "zlib stream is too short".into(),
        ));
    }

    let cmf = data[0];
    let flg = data[1];
    let header = u16::from(cmf) << 8 | u16::from(flg);
    if header % 31 != 0 {
        return Err(PngDecodeError::InvalidData(
            "zlib header check bits are invalid".into(),
        ));
    }
    if cmf & 0x0F != 8 {
        return Err(PngDecodeError::Unsupported(format!(
            "unsupported zlib compression method: {}",
            cmf & 0x0F
        )));
    }
    if cmf >> 4 > 7 {
        return Err(PngDecodeError::Unsupported(
            "zlib window size is too large".into(),
        ));
    }
    if (flg & 0x20) != 0 {
        return Err(PngDecodeError::Unsupported(
            "zlib preset dictionary is not supported".into(),
        ));
    }

    let deflate_bytes = &data[2..data.len() - 4];
    let decoded = deflate::decompress(deflate_bytes)
        .map_err(|error| PngDecodeError::InvalidData(format!("invalid deflate stream: {error}")))?;
    let expected_adler = u32::from_be_bytes(data[data.len() - 4..].try_into().unwrap());
    let actual_adler = adler32::calculate(&decoded);
    if actual_adler != expected_adler {
        return Err(PngDecodeError::InvalidData(
            "zlib adler32 checksum mismatch".into(),
        ));
    }
    Ok(decoded)
}

fn decode_to_rgba(
    header: &PngHeader,
    filtered: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<Vec<u8>, PngDecodeError> {
    if header.interlace_method == 0 {
        let raw = unfilter_scanlines(header, header.width, header.height, filtered)?;
        convert_to_rgba(header, header.width, header.height, &raw, ancillary)
    } else {
        decode_adam7_to_rgba(header, filtered, ancillary)
    }
}

fn unfilter_scanlines(
    header: &PngHeader,
    width: u32,
    height: u32,
    filtered: &[u8],
) -> Result<Vec<u8>, PngDecodeError> {
    let stride = packed_stride_for_width(header, width)?;
    let expected_len = (stride + 1)
        .checked_mul(height as usize)
        .ok_or_else(|| PngDecodeError::InvalidData("filtered data size overflow".into()))?;
    if filtered.len() != expected_len {
        return Err(PngDecodeError::InvalidData(format!(
            "unexpected filtered data size: expected {}, got {}",
            expected_len,
            filtered.len()
        )));
    }

    let bpp = header.filter_bpp();
    let mut raw = vec![0; stride * height as usize];
    for row in 0..height as usize {
        let filter = filtered[row * (stride + 1)];
        let src = &filtered[row * (stride + 1) + 1..(row + 1) * (stride + 1)];
        let row_start = row * stride;
        let (before, current_and_after) = raw.split_at_mut(row_start);
        let dst = &mut current_and_after[..stride];
        let prev = if row == 0 {
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
                return Err(PngDecodeError::InvalidData(format!(
                    "unsupported PNG filter type: {}",
                    filter
                )));
            }
        }
    }
    Ok(raw)
}

fn convert_to_rgba(
    header: &PngHeader,
    width: u32,
    height: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<Vec<u8>, PngDecodeError> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| PngDecodeError::InvalidData("pixel count overflow".into()))?;
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    match (header.color_type, header.bit_depth) {
        (0, 1 | 2 | 4) => decode_grayscale_packed(header, width, raw, ancillary, &mut rgba)?,
        (0, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Grayscale(value)) => Some(value),
                _ => None,
            };
            for &gray in raw {
                let alpha = if Some(u16::from(gray)) == transparent {
                    0
                } else {
                    255
                };
                rgba.extend_from_slice(&[gray, gray, gray, alpha]);
            }
        }
        (0, 16) => decode_grayscale16(raw, ancillary, &mut rgba),
        (2, 8) => {
            let transparent = match ancillary.transparency {
                Some(Transparency::Truecolor(value)) => Some(value),
                _ => None,
            };
            for chunk in raw.chunks_exact(3) {
                let alpha = if Some([
                    u16::from(chunk[0]),
                    u16::from(chunk[1]),
                    u16::from(chunk[2]),
                ]) == transparent
                {
                    0
                } else {
                    255
                };
                rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], alpha]);
            }
        }
        (2, 16) => decode_truecolor16(raw, ancillary, &mut rgba),
        (3, 1 | 2 | 4 | 8) => decode_palette(header, width, raw, ancillary, &mut rgba)?,
        (4, 8) => {
            for chunk in raw.chunks_exact(2) {
                rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
        }
        (4, 16) => decode_grayscale_alpha16(raw, &mut rgba),
        (6, 8) => rgba.extend_from_slice(raw),
        (6, 16) => decode_rgba16(raw, &mut rgba),
        _ => unreachable!(),
    }
    Ok(rgba)
}

fn decode_grayscale_packed(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
    rgba: &mut Vec<u8>,
) -> Result<(), PngDecodeError> {
    let transparent = match ancillary.transparency {
        Some(Transparency::Grayscale(value)) => Some(value),
        _ => None,
    };
    let row_stride = packed_stride_for_width(header, width)?;
    for row in raw.chunks_exact(row_stride) {
        for gray in unpack_samples(row, width as usize, header.bit_depth) {
            let gray_sample = u16::from(gray);
            let gray8 = scale_sample_to_u8(gray_sample, header.bit_depth);
            let alpha = if Some(gray_sample) == transparent {
                0
            } else {
                255
            };
            rgba.extend_from_slice(&[gray8, gray8, gray8, alpha]);
        }
    }
    Ok(())
}

fn decode_grayscale16(raw: &[u8], ancillary: &AncillaryChunks, rgba: &mut Vec<u8>) {
    let transparent = match ancillary.transparency {
        Some(Transparency::Grayscale(value)) => Some(value),
        _ => None,
    };
    for chunk in raw.chunks_exact(2) {
        let gray16 = u16::from_be_bytes([chunk[0], chunk[1]]);
        let gray8 = downsample_u16(gray16);
        let alpha = if Some(gray16) == transparent { 0 } else { 255 };
        rgba.extend_from_slice(&[gray8, gray8, gray8, alpha]);
    }
}

fn decode_truecolor16(raw: &[u8], ancillary: &AncillaryChunks, rgba: &mut Vec<u8>) {
    let transparent = match ancillary.transparency {
        Some(Transparency::Truecolor(value)) => Some(value),
        _ => None,
    };
    for chunk in raw.chunks_exact(6) {
        let rgb16 = [
            u16::from_be_bytes([chunk[0], chunk[1]]),
            u16::from_be_bytes([chunk[2], chunk[3]]),
            u16::from_be_bytes([chunk[4], chunk[5]]),
        ];
        let alpha = if Some(rgb16) == transparent { 0 } else { 255 };
        rgba.extend_from_slice(&[
            downsample_u16(rgb16[0]),
            downsample_u16(rgb16[1]),
            downsample_u16(rgb16[2]),
            alpha,
        ]);
    }
}

fn decode_grayscale_alpha16(raw: &[u8], rgba: &mut Vec<u8>) {
    for chunk in raw.chunks_exact(4) {
        let gray = downsample_u16(u16::from_be_bytes([chunk[0], chunk[1]]));
        let alpha = downsample_u16(u16::from_be_bytes([chunk[2], chunk[3]]));
        rgba.extend_from_slice(&[gray, gray, gray, alpha]);
    }
}

fn decode_rgba16(raw: &[u8], rgba: &mut Vec<u8>) {
    for chunk in raw.chunks_exact(8) {
        rgba.extend_from_slice(&[
            downsample_u16(u16::from_be_bytes([chunk[0], chunk[1]])),
            downsample_u16(u16::from_be_bytes([chunk[2], chunk[3]])),
            downsample_u16(u16::from_be_bytes([chunk[4], chunk[5]])),
            downsample_u16(u16::from_be_bytes([chunk[6], chunk[7]])),
        ]);
    }
}

fn decode_palette(
    header: &PngHeader,
    width: u32,
    raw: &[u8],
    ancillary: &AncillaryChunks,
    rgba: &mut Vec<u8>,
) -> Result<(), PngDecodeError> {
    let palette = ancillary
        .palette
        .as_ref()
        .ok_or_else(|| PngDecodeError::InvalidChunk("missing PLTE for palette image".into()))?;
    let alpha = match ancillary.transparency.as_ref() {
        Some(Transparency::Palette(alpha)) => Some(alpha.as_slice()),
        _ => None,
    };
    let row_stride = packed_stride_for_width(header, width)?;
    for row in raw.chunks_exact(row_stride) {
        for index in unpack_samples(row, width as usize, header.bit_depth) {
            let Some(rgb) = palette.get(index as usize) else {
                return Err(PngDecodeError::InvalidData(format!(
                    "palette index out of range: {}",
                    index
                )));
            };
            let alpha = alpha
                .and_then(|table| table.get(index as usize))
                .copied()
                .unwrap_or(255);
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
        }
    }
    Ok(())
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

fn scale_sample_to_u8(sample: u16, bit_depth: u8) -> u8 {
    if bit_depth == 8 {
        sample as u8
    } else {
        ((u32::from(sample) * 255) / ((1u32 << bit_depth) - 1)) as u8
    }
}

fn downsample_u16(sample: u16) -> u8 {
    (sample >> 8) as u8
}

fn encode_error_into_io(error: PngEncodeError) -> std::io::Error {
    match error {
        PngEncodeError::Io(error) => error,
        PngEncodeError::Unsupported(message) | PngEncodeError::InvalidData(message) => {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
        }
    }
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
    fn from_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
        options: PngEncodeOptions,
    ) -> Result<Self, PngEncodeError> {
        let pixels = rgba
            .chunks_exact(4)
            .map(|chunk| [chunk[0], chunk[1], chunk[2], chunk[3]])
            .collect::<Vec<_>>();
        let target = EncodingTarget::analyze(&pixels, options.color_mode)?;
        let interlace_method = u8::from(options.interlaced);
        let filtered_data = if options.interlaced {
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

impl EncodingTarget {
    fn analyze(pixels: &[[u8; 4]], mode: PngColorMode) -> Result<Self, PngEncodeError> {
        let opaque = pixels.iter().all(|pixel| pixel[3] == 255);
        let grayscale = pixels
            .iter()
            .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]);
        let gray_level_count = pixels
            .iter()
            .map(|pixel| pixel[0])
            .collect::<std::collections::BTreeSet<_>>()
            .len();

        let indexed = analyze_palette(pixels);

        let mode = match mode {
            PngColorMode::Auto => {
                if grayscale && opaque {
                    PngColorMode::Grayscale
                } else if grayscale {
                    PngColorMode::GrayscaleAlpha
                } else if opaque {
                    PngColorMode::Rgb
                } else if indexed.is_some() {
                    PngColorMode::Indexed
                } else {
                    PngColorMode::Rgba
                }
            }
            other => other,
        };

        match mode {
            PngColorMode::Grayscale => {
                if !grayscale || !opaque {
                    return Err(PngEncodeError::Unsupported(
                        "grayscale encoding requires opaque grayscale pixels".into(),
                    ));
                }
                let bit_depth = grayscale_bit_depth(pixels, gray_level_count);
                Ok(Self {
                    bit_depth,
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE,
                    palette: None,
                    trns: None,
                    pixel_kind: if bit_depth < 8 {
                        EncodedPixelKind::GrayscalePacked
                    } else {
                        EncodedPixelKind::Grayscale8
                    },
                })
            }
            PngColorMode::GrayscaleAlpha => {
                if !grayscale {
                    return Err(PngEncodeError::Unsupported(
                        "grayscale+alpha encoding requires grayscale pixels".into(),
                    ));
                }
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE_ALPHA,
                    palette: None,
                    trns: None,
                    pixel_kind: EncodedPixelKind::GrayscaleAlpha8,
                })
            }
            PngColorMode::Rgb => {
                if !opaque {
                    return Err(PngEncodeError::Unsupported(
                        "rgb encoding requires opaque pixels".into(),
                    ));
                }
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_RGB,
                    palette: None,
                    trns: None,
                    pixel_kind: EncodedPixelKind::Rgb8,
                })
            }
            PngColorMode::Rgba => Ok(Self {
                bit_depth: 8,
                color_type: IhdrChunk::COLOR_TYPE_RGBA,
                palette: None,
                trns: None,
                pixel_kind: EncodedPixelKind::Rgba8,
            }),
            PngColorMode::Indexed => {
                let Some(indexed) = indexed else {
                    return Err(PngEncodeError::Unsupported(
                        "indexed encoding requires at most 256 distinct colors".into(),
                    ));
                };
                let bit_depth = indexed_bit_depth(indexed.palette.len());
                Ok(Self {
                    bit_depth,
                    color_type: IhdrChunk::COLOR_TYPE_INDEXED,
                    palette: Some(indexed.palette),
                    trns: indexed.trns,
                    pixel_kind: EncodedPixelKind::Indexed,
                })
            }
            PngColorMode::Auto => unreachable!(),
        }
    }
}

#[derive(Debug)]
struct IndexedAnalysis {
    palette: Vec<[u8; 3]>,
    trns: Option<Vec<u8>>,
}

fn analyze_palette(pixels: &[[u8; 4]]) -> Option<IndexedAnalysis> {
    use std::collections::BTreeMap;

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

fn grayscale_bit_depth(pixels: &[[u8; 4]], gray_level_count: usize) -> u8 {
    if pixels.iter().all(|pixel| matches!(pixel[0], 0 | 255)) {
        1
    } else if pixels
        .iter()
        .all(|pixel| matches!(pixel[0], 0 | 85 | 170 | 255))
    {
        2
    } else if pixels.iter().all(|pixel| pixel[0] % 17 == 0) && gray_level_count <= 16 {
        4
    } else {
        8
    }
}

fn indexed_bit_depth(size: usize) -> u8 {
    match size {
        0 => unreachable!(),
        1..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

fn build_filtered_data(
    width: u32,
    height: u32,
    pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<Vec<u8>, PngEncodeError> {
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
) -> Result<Vec<u8>, PngEncodeError> {
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

fn encode_row_into(
    out: &mut Vec<u8>,
    row_pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<(), PngEncodeError> {
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
            let palette = target.palette.as_ref().expect("palette");
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
                            PngEncodeError::InvalidData("pixel missing from indexed palette".into())
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            pack_samples_to(out, &indices, target.bit_depth);
        }
    }
    Ok(())
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

fn packed_stride_for_width(header: &PngHeader, width: u32) -> Result<usize, PngDecodeError> {
    (width as usize)
        .checked_mul(header.bits_per_pixel())
        .map(|bits| bits.div_ceil(8))
        .ok_or_else(|| PngDecodeError::InvalidData("scanline stride overflow".into()))
}

fn decode_adam7_to_rgba(
    header: &PngHeader,
    filtered: &[u8],
    ancillary: &AncillaryChunks,
) -> Result<Vec<u8>, PngDecodeError> {
    let mut rgba = vec![0; header.width as usize * header.height as usize * 4];
    let mut offset = 0;

    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(header.width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(header.height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }

        let pass_stride = packed_stride_for_width(header, pass_width)?;
        let pass_filtered_len = (pass_stride + 1)
            .checked_mul(pass_height as usize)
            .ok_or_else(|| PngDecodeError::InvalidData("Adam7 pass size overflow".into()))?;
        let pass_filtered = filtered
            .get(offset..offset + pass_filtered_len)
            .ok_or_else(|| PngDecodeError::InvalidData("truncated Adam7 data".into()))?;
        offset += pass_filtered_len;

        let pass_raw = unfilter_scanlines(header, pass_width, pass_height, pass_filtered)?;
        let pass_rgba = convert_to_rgba(header, pass_width, pass_height, &pass_raw, ancillary)?;
        scatter_adam7_pass(
            &mut rgba,
            header.width,
            pass,
            pass_width,
            pass_height,
            &pass_rgba,
        );
    }

    if offset != filtered.len() {
        return Err(PngDecodeError::InvalidData(format!(
            "unexpected Adam7 data size: consumed {}, got {}",
            offset,
            filtered.len()
        )));
    }

    Ok(rgba)
}

fn scatter_adam7_pass(
    full_rgba: &mut [u8],
    full_width: u32,
    pass: Adam7Pass,
    pass_width: u32,
    pass_height: u32,
    pass_rgba: &[u8],
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

    fn read_u32(&mut self) -> Result<u32, PngDecodeError> {
        Ok(u32::from_be_bytes(self.read_array::<4>()?))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], PngDecodeError> {
        let bytes = self.read_bytes(N)?;
        Ok(bytes.try_into().unwrap())
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], PngDecodeError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| PngDecodeError::InvalidData("PNG chunk size overflow".into()))?;
        let Some(bytes) = self.bytes.get(self.offset..end) else {
            return Err(PngDecodeError::InvalidData(
                "unexpected end of PNG stream".into(),
            ));
        };
        self.offset = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{IhdrChunk, PngColorMode, PngEncodeOptions, PngImage};

    #[test]
    fn roundtrip_rgba_writer_and_reader() {
        let image = PngImage::new(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128]).unwrap();
        let mut bytes = Vec::new();
        image.write_to(&mut bytes).unwrap();

        let decoded = PngImage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.width(), 2);
        assert_eq!(decoded.height(), 1);
        assert_eq!(decoded.data(), image.data());
    }

    #[test]
    fn write_to_auto_prefers_grayscale_for_opaque_grayscale() {
        let image = PngImage::new(
            4,
            1,
            vec![
                0, 0, 0, 255, 85, 85, 85, 255, 170, 170, 170, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap();
        let mut bytes = Vec::new();
        image.write_to(&mut bytes).unwrap();

        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 2);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_GRAYSCALE);
        assert_eq!(ihdr.interlace_method, 0);
        assert!(find_chunk(&bytes, b"PLTE").is_none());
        let decoded = PngImage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.data(), image.data());
    }

    #[test]
    fn write_to_with_options_can_force_indexed_and_trns() {
        let image = PngImage::new(
            4,
            1,
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 0, 64,
            ],
        )
        .unwrap();
        let mut bytes = Vec::new();
        image
            .write_to_with_options(
                &mut bytes,
                PngEncodeOptions {
                    color_mode: PngColorMode::Indexed,
                    interlaced: false,
                },
            )
            .unwrap();

        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.bit_depth, 2);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_INDEXED);
        assert!(find_chunk(&bytes, b"PLTE").is_some());
        assert!(find_chunk(&bytes, b"tRNS").is_some());
        let decoded = PngImage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.data(), image.data());
    }

    #[test]
    fn write_to_with_options_can_force_adam7() {
        let image = PngImage::new(
            3,
            3,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 10, 20, 30, 255, 40, 50, 60, 255,
                70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150, 255, 160, 170, 180, 255,
            ],
        )
        .unwrap();
        let mut bytes = Vec::new();
        image
            .write_to_with_options(
                &mut bytes,
                PngEncodeOptions {
                    color_mode: PngColorMode::Rgb,
                    interlaced: true,
                },
            )
            .unwrap();

        let ihdr = read_ihdr(&bytes);
        assert_eq!(ihdr.color_type, IhdrChunk::COLOR_TYPE_RGB);
        assert_eq!(ihdr.interlace_method, 1);
        let decoded = PngImage::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.data(), image.data());
    }

    struct IhdrInfo {
        bit_depth: u8,
        color_type: u8,
        interlace_method: u8,
    }

    fn read_ihdr(bytes: &[u8]) -> IhdrInfo {
        let ihdr = find_chunk(bytes, b"IHDR").unwrap();
        IhdrInfo {
            bit_depth: ihdr[8],
            color_type: ihdr[9],
            interlace_method: ihdr[12],
        }
    }

    fn find_chunk<'a>(bytes: &'a [u8], chunk_type: &[u8; 4]) -> Option<&'a [u8]> {
        let mut offset = 8;
        while offset + 12 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            let current_type: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
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
