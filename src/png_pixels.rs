use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;

use crate::png_types::{Error, PngBitDepth, PngColorMode, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PngPixels<'a> {
    Gray1(Cow<'a, [u8]>),
    Gray2(Cow<'a, [u8]>),
    Gray4(Cow<'a, [u8]>),
    Gray8(Cow<'a, [u8]>),
    Gray16(Cow<'a, [u16]>),
    GrayAlpha8(Cow<'a, [u8]>),
    GrayAlpha16(Cow<'a, [u16]>),
    Rgb8(Cow<'a, [u8]>),
    Rgb16(Cow<'a, [u16]>),
    Rgba8(Cow<'a, [u8]>),
    Rgba16(Cow<'a, [u16]>),
    Indexed1 {
        indices: Cow<'a, [u8]>,
        palette: Cow<'a, [u8]>,
        trns: Option<Cow<'a, [u8]>>,
    },
    Indexed2 {
        indices: Cow<'a, [u8]>,
        palette: Cow<'a, [u8]>,
        trns: Option<Cow<'a, [u8]>>,
    },
    Indexed4 {
        indices: Cow<'a, [u8]>,
        palette: Cow<'a, [u8]>,
        trns: Option<Cow<'a, [u8]>>,
    },
    Indexed8 {
        indices: Cow<'a, [u8]>,
        palette: Cow<'a, [u8]>,
        trns: Option<Cow<'a, [u8]>>,
    },
}

impl<'a> PngPixels<'a> {
    pub fn from_gray1(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::Gray1(data.into())
    }

    pub fn from_gray2(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::Gray2(data.into())
    }

    pub fn from_gray4(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::Gray4(data.into())
    }

    pub fn from_gray8(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::Gray8(data.into())
    }

    pub fn from_gray16(data: impl Into<Cow<'a, [u16]>>) -> Self {
        Self::Gray16(data.into())
    }

    pub fn from_gray_alpha8(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::GrayAlpha8(data.into())
    }

    pub fn from_gray_alpha16(data: impl Into<Cow<'a, [u16]>>) -> Self {
        Self::GrayAlpha16(data.into())
    }

    pub fn from_rgb8(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::Rgb8(data.into())
    }

    pub fn from_rgb16(data: impl Into<Cow<'a, [u16]>>) -> Self {
        Self::Rgb16(data.into())
    }

    pub fn from_rgba8(data: impl Into<Cow<'a, [u8]>>) -> Self {
        Self::Rgba8(data.into())
    }

    pub fn from_rgba16(data: impl Into<Cow<'a, [u16]>>) -> Self {
        Self::Rgba16(data.into())
    }

    pub fn from_indexed1<I, P, T>(indices: I, palette: P, trns: Option<T>) -> Self
    where
        I: Into<Cow<'a, [u8]>>,
        P: Into<Cow<'a, [u8]>>,
        T: Into<Cow<'a, [u8]>>,
    {
        Self::Indexed1 {
            indices: indices.into(),
            palette: palette.into(),
            trns: trns.map(Into::into),
        }
    }

    pub fn from_indexed2<I, P, T>(indices: I, palette: P, trns: Option<T>) -> Self
    where
        I: Into<Cow<'a, [u8]>>,
        P: Into<Cow<'a, [u8]>>,
        T: Into<Cow<'a, [u8]>>,
    {
        Self::Indexed2 {
            indices: indices.into(),
            palette: palette.into(),
            trns: trns.map(Into::into),
        }
    }

    pub fn from_indexed4<I, P, T>(indices: I, palette: P, trns: Option<T>) -> Self
    where
        I: Into<Cow<'a, [u8]>>,
        P: Into<Cow<'a, [u8]>>,
        T: Into<Cow<'a, [u8]>>,
    {
        Self::Indexed4 {
            indices: indices.into(),
            palette: palette.into(),
            trns: trns.map(Into::into),
        }
    }

    pub fn from_indexed8<I, P, T>(indices: I, palette: P, trns: Option<T>) -> Self
    where
        I: Into<Cow<'a, [u8]>>,
        P: Into<Cow<'a, [u8]>>,
        T: Into<Cow<'a, [u8]>>,
    {
        Self::Indexed8 {
            indices: indices.into(),
            palette: palette.into(),
            trns: trns.map(Into::into),
        }
    }

    pub fn pixel_count(&self) -> usize {
        match self {
            Self::Gray1(data) | Self::Gray2(data) | Self::Gray4(data) | Self::Gray8(data) => {
                data.len()
            }
            Self::Gray16(data) => data.len(),
            Self::GrayAlpha8(data) => data.len() / 2,
            Self::GrayAlpha16(data) => data.len() / 2,
            Self::Rgb8(data) => data.len() / 3,
            Self::Rgb16(data) => data.len() / 3,
            Self::Rgba8(data) => data.len() / 4,
            Self::Rgba16(data) => data.len() / 4,
            Self::Indexed1 { indices, .. }
            | Self::Indexed2 { indices, .. }
            | Self::Indexed4 { indices, .. }
            | Self::Indexed8 { indices, .. } => indices.len(),
        }
    }

    pub fn color_mode(&self) -> PngColorMode {
        match self {
            Self::Gray1(_) | Self::Gray2(_) | Self::Gray4(_) | Self::Gray8(_) | Self::Gray16(_) => {
                PngColorMode::Grayscale
            }
            Self::GrayAlpha8(_) | Self::GrayAlpha16(_) => PngColorMode::GrayscaleAlpha,
            Self::Rgb8(_) | Self::Rgb16(_) => PngColorMode::Rgb,
            Self::Rgba8(_) | Self::Rgba16(_) => PngColorMode::Rgba,
            Self::Indexed1 { .. }
            | Self::Indexed2 { .. }
            | Self::Indexed4 { .. }
            | Self::Indexed8 { .. } => PngColorMode::Indexed,
        }
    }

    pub fn bit_depth(&self) -> PngBitDepth {
        match self {
            Self::Gray1(_) | Self::Indexed1 { .. } => PngBitDepth::One,
            Self::Gray2(_) | Self::Indexed2 { .. } => PngBitDepth::Two,
            Self::Gray4(_) | Self::Indexed4 { .. } => PngBitDepth::Four,
            Self::Gray8(_)
            | Self::GrayAlpha8(_)
            | Self::Rgb8(_)
            | Self::Rgba8(_)
            | Self::Indexed8 { .. } => PngBitDepth::Eight,
            Self::Gray16(_) | Self::GrayAlpha16(_) | Self::Rgb16(_) | Self::Rgba16(_) => {
                PngBitDepth::Sixteen
            }
        }
    }

    pub fn as_u8_slice(&self) -> Option<&[u8]> {
        match self {
            Self::Gray1(data)
            | Self::Gray2(data)
            | Self::Gray4(data)
            | Self::Gray8(data)
            | Self::GrayAlpha8(data)
            | Self::Rgb8(data)
            | Self::Rgba8(data) => Some(data.as_ref()),
            _ => None,
        }
    }

    pub fn as_u16_slice(&self) -> Option<&[u16]> {
        match self {
            Self::Gray16(data)
            | Self::GrayAlpha16(data)
            | Self::Rgb16(data)
            | Self::Rgba16(data) => Some(data.as_ref()),
            _ => None,
        }
    }

    pub fn indices(&self) -> Option<&[u8]> {
        match self {
            Self::Indexed1 { indices, .. }
            | Self::Indexed2 { indices, .. }
            | Self::Indexed4 { indices, .. }
            | Self::Indexed8 { indices, .. } => Some(indices.as_ref()),
            _ => None,
        }
    }

    pub fn palette(&self) -> Option<&[u8]> {
        match self {
            Self::Indexed1 { palette, .. }
            | Self::Indexed2 { palette, .. }
            | Self::Indexed4 { palette, .. }
            | Self::Indexed8 { palette, .. } => Some(palette.as_ref()),
            _ => None,
        }
    }

    pub fn trns(&self) -> Option<&[u8]> {
        match self {
            Self::Indexed1 { trns, .. }
            | Self::Indexed2 { trns, .. }
            | Self::Indexed4 { trns, .. }
            | Self::Indexed8 { trns, .. } => trns.as_deref(),
            _ => None,
        }
    }

    pub fn to_owned(&self) -> PngPixels<'static> {
        match self {
            Self::Gray1(data) => PngPixels::Gray1(Cow::Owned(data.to_vec())),
            Self::Gray2(data) => PngPixels::Gray2(Cow::Owned(data.to_vec())),
            Self::Gray4(data) => PngPixels::Gray4(Cow::Owned(data.to_vec())),
            Self::Gray8(data) => PngPixels::Gray8(Cow::Owned(data.to_vec())),
            Self::Gray16(data) => PngPixels::Gray16(Cow::Owned(data.to_vec())),
            Self::GrayAlpha8(data) => PngPixels::GrayAlpha8(Cow::Owned(data.to_vec())),
            Self::GrayAlpha16(data) => PngPixels::GrayAlpha16(Cow::Owned(data.to_vec())),
            Self::Rgb8(data) => PngPixels::Rgb8(Cow::Owned(data.to_vec())),
            Self::Rgb16(data) => PngPixels::Rgb16(Cow::Owned(data.to_vec())),
            Self::Rgba8(data) => PngPixels::Rgba8(Cow::Owned(data.to_vec())),
            Self::Rgba16(data) => PngPixels::Rgba16(Cow::Owned(data.to_vec())),
            Self::Indexed1 { indices, palette, trns } => PngPixels::Indexed1 {
                indices: Cow::Owned(indices.to_vec()),
                palette: Cow::Owned(palette.to_vec()),
                trns: trns.as_ref().map(|value| Cow::Owned(value.to_vec())),
            },
            Self::Indexed2 { indices, palette, trns } => PngPixels::Indexed2 {
                indices: Cow::Owned(indices.to_vec()),
                palette: Cow::Owned(palette.to_vec()),
                trns: trns.as_ref().map(|value| Cow::Owned(value.to_vec())),
            },
            Self::Indexed4 { indices, palette, trns } => PngPixels::Indexed4 {
                indices: Cow::Owned(indices.to_vec()),
                palette: Cow::Owned(palette.to_vec()),
                trns: trns.as_ref().map(|value| Cow::Owned(value.to_vec())),
            },
            Self::Indexed8 { indices, palette, trns } => PngPixels::Indexed8 {
                indices: Cow::Owned(indices.to_vec()),
                palette: Cow::Owned(palette.to_vec()),
                trns: trns.as_ref().map(|value| Cow::Owned(value.to_vec())),
            },
        }
    }

    pub fn into_owned(self) -> PngPixels<'static> {
        match self {
            Self::Gray1(data) => PngPixels::Gray1(Cow::Owned(data.into_owned())),
            Self::Gray2(data) => PngPixels::Gray2(Cow::Owned(data.into_owned())),
            Self::Gray4(data) => PngPixels::Gray4(Cow::Owned(data.into_owned())),
            Self::Gray8(data) => PngPixels::Gray8(Cow::Owned(data.into_owned())),
            Self::Gray16(data) => PngPixels::Gray16(Cow::Owned(data.into_owned())),
            Self::GrayAlpha8(data) => PngPixels::GrayAlpha8(Cow::Owned(data.into_owned())),
            Self::GrayAlpha16(data) => PngPixels::GrayAlpha16(Cow::Owned(data.into_owned())),
            Self::Rgb8(data) => PngPixels::Rgb8(Cow::Owned(data.into_owned())),
            Self::Rgb16(data) => PngPixels::Rgb16(Cow::Owned(data.into_owned())),
            Self::Rgba8(data) => PngPixels::Rgba8(Cow::Owned(data.into_owned())),
            Self::Rgba16(data) => PngPixels::Rgba16(Cow::Owned(data.into_owned())),
            Self::Indexed1 { indices, palette, trns } => PngPixels::Indexed1 {
                indices: Cow::Owned(indices.into_owned()),
                palette: Cow::Owned(palette.into_owned()),
                trns: trns.map(|value| Cow::Owned(value.into_owned())),
            },
            Self::Indexed2 { indices, palette, trns } => PngPixels::Indexed2 {
                indices: Cow::Owned(indices.into_owned()),
                palette: Cow::Owned(palette.into_owned()),
                trns: trns.map(|value| Cow::Owned(value.into_owned())),
            },
            Self::Indexed4 { indices, palette, trns } => PngPixels::Indexed4 {
                indices: Cow::Owned(indices.into_owned()),
                palette: Cow::Owned(palette.into_owned()),
                trns: trns.map(|value| Cow::Owned(value.into_owned())),
            },
            Self::Indexed8 { indices, palette, trns } => PngPixels::Indexed8 {
                indices: Cow::Owned(indices.into_owned()),
                palette: Cow::Owned(palette.into_owned()),
                trns: trns.map(|value| Cow::Owned(value.into_owned())),
            },
        }
    }

    pub fn to_gray8(&self) -> PngPixels<'static> {
        PngPixels::Gray8(Cow::Owned(self.to_gray8_vec()))
    }

    pub fn to_gray16(&self) -> PngPixels<'static> {
        PngPixels::Gray16(Cow::Owned(self.to_gray16_vec()))
    }

    pub fn to_rgb8(&self) -> PngPixels<'static> {
        PngPixels::Rgb8(Cow::Owned(self.to_rgb8_vec()))
    }

    pub fn to_rgb16(&self) -> PngPixels<'static> {
        PngPixels::Rgb16(Cow::Owned(self.to_rgb16_vec()))
    }

    pub fn to_rgba8(&self) -> PngPixels<'static> {
        PngPixels::Rgba8(Cow::Owned(self.to_rgba8_vec()))
    }

    pub fn to_rgba16(&self) -> PngPixels<'static> {
        PngPixels::Rgba16(Cow::Owned(self.to_rgba16_vec()))
    }

    pub(crate) fn to_gray8_vec(&self) -> Vec<u8> {
        match self {
            Self::Gray1(data) => data.iter().map(|&sample| scale_sample_to_u8(u16::from(sample), 1)).collect(),
            Self::Gray2(data) => data.iter().map(|&sample| scale_sample_to_u8(u16::from(sample), 2)).collect(),
            Self::Gray4(data) => data.iter().map(|&sample| scale_sample_to_u8(u16::from(sample), 4)).collect(),
            Self::Gray8(data) => data.to_vec(),
            Self::Gray16(data) => data.iter().copied().map(downsample_u16).collect(),
            Self::GrayAlpha8(data) => {
                let (pairs, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                pairs.iter().map(|[gray, _]| *gray).collect()
            }
            Self::GrayAlpha16(data) => {
                let (pairs, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                pairs.iter().map(|[gray, _]| downsample_u16(*gray)).collect()
            }
            Self::Rgb8(_) | Self::Rgb16(_) | Self::Rgba8(_) | Self::Rgba16(_) | Self::Indexed1 { .. } | Self::Indexed2 { .. } | Self::Indexed4 { .. } | Self::Indexed8 { .. } => {
                let rgba = self.to_rgba8_vec();
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels.iter().map(|[r, g, b, _]| rgb_to_gray8(*r, *g, *b)).collect()
            }
        }
    }

    pub(crate) fn to_gray16_vec(&self) -> Vec<u16> {
        match self {
            Self::Gray16(data) => data.to_vec(),
            Self::GrayAlpha16(data) => {
                let (pairs, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                pairs.iter().map(|[gray, _]| *gray).collect()
            }
            Self::Rgb16(_) | Self::Rgba16(_) => {
                let rgba = self.to_rgba16_vec();
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels.iter().map(|[r, g, b, _]| rgb_to_gray16(*r, *g, *b)).collect()
            }
            _ => self.to_gray8_vec().into_iter().map(upscale_u8_to_u16).collect(),
        }
    }

    pub(crate) fn to_rgb8_vec(&self) -> Vec<u8> {
        match self {
            Self::Rgb8(data) => data.to_vec(),
            Self::Rgb16(data) => data.iter().copied().map(downsample_u16).collect(),
            Self::Gray1(_) | Self::Gray2(_) | Self::Gray4(_) | Self::Gray8(_) | Self::Gray16(_) | Self::GrayAlpha8(_) | Self::GrayAlpha16(_) => {
                let gray = self.to_gray8_vec();
                let mut rgb = Vec::with_capacity(gray.len() * 3);
                for value in gray {
                    rgb.extend_from_slice(&[value, value, value]);
                }
                rgb
            }
            Self::Rgba8(data) => {
                let (pixels, remainder) = data.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect()
            }
            Self::Rgba16(data) => {
                let (pixels, remainder) = data.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels.iter().flat_map(|[r, g, b, _]| [downsample_u16(*r), downsample_u16(*g), downsample_u16(*b)]).collect()
            }
            Self::Indexed1 { .. } | Self::Indexed2 { .. } | Self::Indexed4 { .. } | Self::Indexed8 { .. } => {
                let rgba = self.to_rgba8_vec();
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect()
            }
        }
    }

    pub(crate) fn to_rgb16_vec(&self) -> Vec<u16> {
        match self {
            Self::Rgb16(data) => data.to_vec(),
            Self::Rgba16(data) => {
                let (pixels, remainder) = data.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect()
            }
            _ => self.to_rgb8_vec().into_iter().map(upscale_u8_to_u16).collect(),
        }
    }

    pub(crate) fn to_rgba8_vec(&self) -> Vec<u8> {
        match self {
            Self::Rgba8(data) => data.to_vec(),
            Self::Rgba16(data) => data.iter().copied().map(downsample_u16).collect(),
            Self::Rgb8(data) => {
                let mut rgba = Vec::with_capacity(data.len() / 3 * 4);
                let (pixels, remainder) = data.as_chunks::<3>();
                debug_assert!(remainder.is_empty());
                for &[r, g, b] in pixels {
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
                rgba
            }
            Self::Rgb16(data) => {
                let mut rgba = Vec::with_capacity(data.len() / 3 * 4);
                let (pixels, remainder) = data.as_chunks::<3>();
                debug_assert!(remainder.is_empty());
                for &[r, g, b] in pixels {
                    rgba.extend_from_slice(&[downsample_u16(r), downsample_u16(g), downsample_u16(b), 255]);
                }
                rgba
            }
            Self::Gray1(_) | Self::Gray2(_) | Self::Gray4(_) | Self::Gray8(_) | Self::Gray16(_) => {
                let gray = self.to_gray8_vec();
                let mut rgba = Vec::with_capacity(gray.len() * 4);
                for value in gray {
                    rgba.extend_from_slice(&[value, value, value, 255]);
                }
                rgba
            }
            Self::GrayAlpha8(data) => {
                let mut rgba = Vec::with_capacity(data.len() / 2 * 4);
                let (pixels, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                for &[gray, alpha] in pixels {
                    rgba.extend_from_slice(&[gray, gray, gray, alpha]);
                }
                rgba
            }
            Self::GrayAlpha16(data) => {
                let mut rgba = Vec::with_capacity(data.len() / 2 * 4);
                let (pixels, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                for &[gray, alpha] in pixels {
                    rgba.extend_from_slice(&[downsample_u16(gray), downsample_u16(gray), downsample_u16(gray), downsample_u16(alpha)]);
                }
                rgba
            }
            Self::Indexed1 { .. } | Self::Indexed2 { .. } | Self::Indexed4 { .. } | Self::Indexed8 { .. } => indexed_to_rgba8(self),
        }
    }

    pub(crate) fn to_rgba16_vec(&self) -> Vec<u16> {
        match self {
            Self::Rgba16(data) => data.to_vec(),
            Self::Rgb16(data) => {
                let mut rgba = Vec::with_capacity(data.len() / 3 * 4);
                let (pixels, remainder) = data.as_chunks::<3>();
                debug_assert!(remainder.is_empty());
                for &[r, g, b] in pixels {
                    rgba.extend_from_slice(&[r, g, b, u16::MAX]);
                }
                rgba
            }
            Self::Gray16(data) => {
                let mut rgba = Vec::with_capacity(data.len() * 4);
                for &value in data.iter() {
                    rgba.extend_from_slice(&[value, value, value, u16::MAX]);
                }
                rgba
            }
            Self::GrayAlpha16(data) => {
                let mut rgba = Vec::with_capacity(data.len() / 2 * 4);
                let (pixels, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                for &[gray, alpha] in pixels {
                    rgba.extend_from_slice(&[gray, gray, gray, alpha]);
                }
                rgba
            }
            _ => self.to_rgba8_vec().into_iter().map(upscale_u8_to_u16).collect(),
        }
    }
}

pub(crate) fn validate_pixels(pixels: &PngPixels<'_>) -> Result<()> {
    match pixels {
        PngPixels::Gray1(data) => validate_sample_range(data, 1, "Gray1"),
        PngPixels::Gray2(data) => validate_sample_range(data, 2, "Gray2"),
        PngPixels::Gray4(data) => validate_sample_range(data, 4, "Gray4"),
        PngPixels::Gray8(_)
        | PngPixels::Gray16(_)
        | PngPixels::GrayAlpha8(_)
        | PngPixels::GrayAlpha16(_)
        | PngPixels::Rgb8(_)
        | PngPixels::Rgb16(_)
        | PngPixels::Rgba8(_)
        | PngPixels::Rgba16(_) => Ok(()),
        PngPixels::Indexed1 { indices, palette, trns } => {
            validate_indexed_pixels(indices, palette, trns.as_deref(), 1)
        }
        PngPixels::Indexed2 { indices, palette, trns } => {
            validate_indexed_pixels(indices, palette, trns.as_deref(), 2)
        }
        PngPixels::Indexed4 { indices, palette, trns } => {
            validate_indexed_pixels(indices, palette, trns.as_deref(), 4)
        }
        PngPixels::Indexed8 { indices, palette, trns } => {
            validate_indexed_pixels(indices, palette, trns.as_deref(), 8)
        }
    }
}

fn validate_sample_range(samples: &[u8], bit_depth: u8, name: &str) -> Result<()> {
    let max = (1u16 << bit_depth) - 1;
    if samples.iter().all(|&sample| u16::from(sample) <= max) {
        Ok(())
    } else {
        Err(Error::InvalidData(format!(
            "{name} contains a sample that does not fit in {bit_depth} bits"
        ).into()))
    }
}

fn validate_indexed_pixels(indices: &[u8], palette: &[u8], trns: Option<&[u8]>, bit_depth: u8) -> Result<()> {
    if palette.is_empty() || !palette.len().is_multiple_of(3) {
        return Err(Error::InvalidData(
            "indexed palette length must be a non-zero multiple of 3".into(),
        ));
    }
    let palette_len = palette.len() / 3;
    if palette_len > 256 {
        return Err(Error::InvalidData(
            "indexed palette length must be in 1..=256".into(),
        ));
    }
    if let Some(trns) = trns && trns.len() > palette_len {
        return Err(Error::InvalidData(
            "indexed transparency table is longer than the palette".into(),
        ));
    }
    let capacity = 1usize << bit_depth;
    if palette_len > capacity {
        return Err(Error::InvalidData(format!(
            "palette of size {} does not fit in {}-bit indexed pixels",
            palette_len, bit_depth
        ).into()));
    }
    if indices.iter().all(|&index| usize::from(index) < palette_len) {
        Ok(())
    } else {
        Err(Error::InvalidData(
            "indexed pixel buffer contains an out-of-range palette index".into(),
        ))
    }
}

pub(crate) fn indexed_to_rgba8(pixels: &PngPixels<'_>) -> Vec<u8> {
    let (indices, palette, trns) = match pixels {
        PngPixels::Indexed1 { indices, palette, trns }
        | PngPixels::Indexed2 { indices, palette, trns }
        | PngPixels::Indexed4 { indices, palette, trns }
        | PngPixels::Indexed8 { indices, palette, trns } => {
            (indices.as_ref(), palette.as_ref(), trns.as_deref())
        }
        _ => unreachable!(),
    };
    let mut rgba = Vec::with_capacity(indices.len() * 4);
    for &index in indices {
        let rgb_index = index as usize * 3;
        let rgb = &palette[rgb_index..rgb_index + 3];
        let alpha = trns
            .and_then(|table| table.get(index as usize))
            .copied()
            .unwrap_or(255);
        rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
    }
    rgba
}

pub(crate) fn flatten_palette(palette: &[[u8; 3]]) -> Vec<u8> {
    let mut flattened = Vec::with_capacity(palette.len() * 3);
    for &[r, g, b] in palette {
        flattened.extend_from_slice(&[r, g, b]);
    }
    flattened
}

fn rgb_to_gray8(r: u8, g: u8, b: u8) -> u8 {
    (((u32::from(r) * 299) + (u32::from(g) * 587) + (u32::from(b) * 114) + 500) / 1000) as u8
}

fn rgb_to_gray16(r: u16, g: u16, b: u16) -> u16 {
    (((u32::from(r) * 299) + (u32::from(g) * 587) + (u32::from(b) * 114) + 500) / 1000) as u16
}

pub(crate) fn scale_sample_to_u8(sample: u16, bit_depth: u8) -> u8 {
    if bit_depth == 8 {
        sample as u8
    } else {
        ((u32::from(sample) * 255) / ((1u32 << bit_depth) - 1)) as u8
    }
}

pub(crate) fn downsample_u16(sample: u16) -> u8 {
    (sample >> 8) as u8
}

pub(crate) fn upscale_u8_to_u16(sample: u8) -> u16 {
    u16::from(sample) * 257
}
