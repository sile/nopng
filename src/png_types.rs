use alloc::borrow::Cow;
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

    pub(crate) fn effective_for_rgba8(self) -> Self {
        match self {
            Self::Sixteen => Self::Eight,
            other => other,
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
    pub(crate) fn from_color_type(color_type: u8) -> Self {
        match color_type {
            0 => Self::Grayscale,
            2 => Self::Rgb,
            3 => Self::Indexed,
            4 => Self::GrayscaleAlpha,
            6 => Self::Rgba,
            _ => unreachable!(),
        }
    }

    pub(crate) fn to_color_type(self) -> u8 {
        match self {
            Self::Grayscale => 0,
            Self::Rgb => 2,
            Self::Indexed => 3,
            Self::GrayscaleAlpha => 4,
            Self::Rgba => 6,
        }
    }

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
