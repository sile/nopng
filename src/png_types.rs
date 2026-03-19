use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::error::Error as CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unsupported(Cow<'static, str>),
    InvalidData(Cow<'static, str>),
}

pub type Result<T> = core::result::Result<T, Error>;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(message) | Self::InvalidData(message) => f.write_str(message),
        }
    }
}

impl CoreError for Error {
    fn source(&self) -> Option<&(dyn CoreError + 'static)> {
        match self {
            Self::Unsupported(_) | Self::InvalidData(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    One,
    Two,
    Four,
    Eight,
    Sixteen,
}

impl BitDepth {
    pub(crate) fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            4 => Some(Self::Four),
            8 => Some(Self::Eight),
            16 => Some(Self::Sixteen),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Grayscale,
    GrayscaleAlpha,
    Rgb,
    Rgba,
    Indexed,
}

impl ColorMode {
    pub fn has_alpha(&self) -> bool {
        matches!(self, Self::GrayscaleAlpha | Self::Rgba)
    }

    pub fn channels(&self) -> u8 {
        match self {
            Self::Grayscale | Self::Indexed => 1,
            Self::GrayscaleAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }
}

/// Describes the pixel layout of image data in a flat `&[u8]` buffer.
///
/// 16-bit samples are stored in big-endian byte order (matching the PNG wire
/// format). Low-bit grayscale and indexed formats use one unpacked byte per
/// sample/index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    /// Grayscale with 1/2/4/8-bit samples (1 byte per sample, unpacked) or
    /// 16-bit samples (2 bytes per sample, big-endian).
    Gray { bit_depth: BitDepth },
    /// Grayscale + alpha, 8-bit (2 bytes per pixel: `[gray, alpha, ...]`).
    GrayAlpha8,
    /// Grayscale + alpha, 16-bit BE (4 bytes per pixel: `[g_hi, g_lo, a_hi, a_lo, ...]`).
    GrayAlpha16Be,
    /// RGB, 8-bit (3 bytes per pixel).
    Rgb8,
    /// RGB, 16-bit BE (6 bytes per pixel).
    Rgb16Be,
    /// RGBA, 8-bit (4 bytes per pixel).
    Rgba8,
    /// RGBA, 16-bit BE (8 bytes per pixel).
    Rgba16Be,
    /// Indexed color with unpacked 1-byte indices and an embedded palette.
    Indexed {
        bit_depth: BitDepth,
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`).
        palette: Vec<u8>,
        /// Per-index alpha values (may be shorter than the palette).
        trns: Option<Vec<u8>>,
    },
}

impl PixelFormat {
    /// Returns the bit depth of each sample.
    pub fn bit_depth(&self) -> BitDepth {
        match self {
            Self::Gray { bit_depth } | Self::Indexed { bit_depth, .. } => *bit_depth,
            Self::GrayAlpha8 | Self::Rgb8 | Self::Rgba8 => BitDepth::Eight,
            Self::GrayAlpha16Be | Self::Rgb16Be | Self::Rgba16Be => BitDepth::Sixteen,
        }
    }

    /// Returns the color mode.
    pub fn color_mode(&self) -> ColorMode {
        match self {
            Self::Gray { .. } => ColorMode::Grayscale,
            Self::GrayAlpha8 | Self::GrayAlpha16Be => ColorMode::GrayscaleAlpha,
            Self::Rgb8 | Self::Rgb16Be => ColorMode::Rgb,
            Self::Rgba8 | Self::Rgba16Be => ColorMode::Rgba,
            Self::Indexed { .. } => ColorMode::Indexed,
        }
    }

    /// Bytes per pixel in the flat buffer.
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Gray { bit_depth } => {
                if *bit_depth == BitDepth::Sixteen {
                    2
                } else {
                    1
                }
            }
            Self::GrayAlpha8 => 2,
            Self::GrayAlpha16Be => 4,
            Self::Rgb8 => 3,
            Self::Rgb16Be => 6,
            Self::Rgba8 => 4,
            Self::Rgba16Be => 8,
            Self::Indexed { .. } => 1,
        }
    }

    /// Expected byte length of the pixel data buffer for the given dimensions.
    pub fn data_len(&self, width: u32, height: u32) -> Result<usize> {
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| Error::InvalidData("pixel count overflow".into()))?;
        pixels
            .checked_mul(self.bytes_per_pixel())
            .ok_or_else(|| Error::InvalidData("data length overflow".into()))
    }

    /// Converts pixel data from this format to `dst_format`.
    pub fn reformat(&self, src: &[u8], dst_format: &PixelFormat) -> Result<Vec<u8>> {
        crate::pixel_reformat::reformat(self, src, dst_format)
    }

    /// Converts pixel data from this format to `dst_format`, writing into `dst`.
    pub fn reformat_into(
        &self,
        src: &[u8],
        dst_format: &PixelFormat,
        dst: &mut [u8],
    ) -> Result<()> {
        crate::pixel_reformat::reformat_into(self, src, dst_format, dst)
    }
}
