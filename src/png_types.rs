use alloc::borrow::Cow;
use core::error::Error as CoreError;

#[derive(Debug, PartialEq, Eq)]
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
pub enum PngBitDepth {
    One,
    Two,
    Four,
    Eight,
    Sixteen,
}

impl PngBitDepth {
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

    pub(crate) fn as_u8(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
            Self::Sixteen => 16,
        }
    }

    pub(crate) fn effective_for_rgba8(self) -> Self {
        match self {
            Self::Sixteen => Self::Eight,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngColorMode {
    Grayscale,
    GrayscaleAlpha,
    Rgb,
    Rgba,
    Indexed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngEncoding {
    pub color_mode: PngColorMode,
    pub bit_depth: PngBitDepth,
    pub interlaced: bool,
}

impl Default for PngEncoding {
    fn default() -> Self {
        Self {
            color_mode: PngColorMode::Rgba,
            bit_depth: PngBitDepth::Eight,
            interlaced: false,
        }
    }
}

/// Basic PNG information read from the PNG signature and `IHDR` chunk.
///
/// This type is intended for cheap preflight checks such as rejecting images
/// whose dimensions are too large for the caller's policy. It does not perform
/// full PNG validation and does not inspect later chunks such as `PLTE`, `tRNS`
/// or `IDAT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: PngBitDepth,
    pub color_mode: PngColorMode,
    pub interlaced: bool,
}

/// PNG image with explicit dimensions, pixel storage and PNG output settings.
#[derive(Debug, Clone)]
pub struct PngImage<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: crate::png_pixels::PngPixels<'a>,
    pub(crate) encoding: PngEncoding,
}
