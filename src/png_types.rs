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

/// Describes the pixel layout of image data in a flat `&[u8]` buffer.
///
/// 16-bit samples are stored in big-endian byte order (matching the PNG wire
/// format). Low-bit grayscale and indexed formats use one unpacked byte per
/// sample/index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    /// 1-bit grayscale, 1 byte per sample (unpacked).
    Gray1,
    /// 2-bit grayscale, 1 byte per sample (unpacked).
    Gray2,
    /// 4-bit grayscale, 1 byte per sample (unpacked).
    Gray4,
    /// 8-bit grayscale, 1 byte per sample.
    Gray8,
    /// 16-bit grayscale, 2 bytes per sample (big-endian).
    Gray16Be,
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
    /// 1-bit indexed color with unpacked 1-byte indices and an embedded palette.
    Indexed1 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`).
        palette: Vec<u8>,
        /// Per-index alpha values (may be shorter than the palette).
        trns: Option<Vec<u8>>,
    },
    /// 2-bit indexed color with unpacked 1-byte indices and an embedded palette.
    Indexed2 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`).
        palette: Vec<u8>,
        /// Per-index alpha values (may be shorter than the palette).
        trns: Option<Vec<u8>>,
    },
    /// 4-bit indexed color with unpacked 1-byte indices and an embedded palette.
    Indexed4 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`).
        palette: Vec<u8>,
        /// Per-index alpha values (may be shorter than the palette).
        trns: Option<Vec<u8>>,
    },
    /// 8-bit indexed color with unpacked 1-byte indices and an embedded palette.
    Indexed8 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`).
        palette: Vec<u8>,
        /// Per-index alpha values (may be shorter than the palette).
        trns: Option<Vec<u8>>,
    },
}

impl PixelFormat {
    /// Returns `true` if this format includes an alpha channel.
    pub fn has_alpha(&self) -> bool {
        matches!(
            self,
            Self::GrayAlpha8 | Self::GrayAlpha16Be | Self::Rgba8 | Self::Rgba16Be
        )
    }

    /// Number of channels per pixel.
    pub fn channels(&self) -> u8 {
        match self {
            Self::Gray1
            | Self::Gray2
            | Self::Gray4
            | Self::Gray8
            | Self::Gray16Be
            | Self::Indexed1 { .. }
            | Self::Indexed2 { .. }
            | Self::Indexed4 { .. }
            | Self::Indexed8 { .. } => 1,
            Self::GrayAlpha8 | Self::GrayAlpha16Be => 2,
            Self::Rgb8 | Self::Rgb16Be => 3,
            Self::Rgba8 | Self::Rgba16Be => 4,
        }
    }

    /// Returns `true` if this is an indexed (palette-based) format.
    pub fn is_indexed(&self) -> bool {
        matches!(
            self,
            Self::Indexed1 { .. }
                | Self::Indexed2 { .. }
                | Self::Indexed4 { .. }
                | Self::Indexed8 { .. }
        )
    }

    /// Bit depth of each sample as a raw `u8`.
    pub fn bit_depth_u8(&self) -> u8 {
        match self {
            Self::Gray1 | Self::Indexed1 { .. } => 1,
            Self::Gray2 | Self::Indexed2 { .. } => 2,
            Self::Gray4 | Self::Indexed4 { .. } => 4,
            Self::Gray8 | Self::GrayAlpha8 | Self::Rgb8 | Self::Rgba8 | Self::Indexed8 { .. } => 8,
            Self::Gray16Be | Self::GrayAlpha16Be | Self::Rgb16Be | Self::Rgba16Be => 16,
        }
    }

    /// Bytes per pixel in the flat buffer.
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Gray1
            | Self::Gray2
            | Self::Gray4
            | Self::Gray8
            | Self::Indexed1 { .. }
            | Self::Indexed2 { .. }
            | Self::Indexed4 { .. }
            | Self::Indexed8 { .. } => 1,
            Self::Gray16Be | Self::GrayAlpha8 => 2,
            Self::Rgb8 => 3,
            Self::GrayAlpha16Be | Self::Rgba8 => 4,
            Self::Rgb16Be => 6,
            Self::Rgba16Be => 8,
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
