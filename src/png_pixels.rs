use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec::Vec;

use crate::png_types::{BitDepth, ColorMode, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pixels<'a> {
    Gray {
        bit_depth: BitDepth,
        samples: Cow<'a, [u8]>,
    },
    Gray16(Cow<'a, [u16]>),
    GrayAlpha8(Cow<'a, [u8]>),
    GrayAlpha16(Cow<'a, [u16]>),
    Rgb8(Cow<'a, [u8]>),
    Rgb16(Cow<'a, [u16]>),
    Rgba8(Cow<'a, [u8]>),
    Rgba16(Cow<'a, [u16]>),
    Indexed {
        bit_depth: BitDepth,
        indices: Cow<'a, [u8]>,
        palette: Cow<'a, [u8]>,
        trns: Option<Cow<'a, [u8]>>,
    },
}

impl<'a> Pixels<'a> {
    pub fn infer_from_rgb8(data: impl Into<Cow<'a, [u8]>>) -> Self {
        let data = data.into();
        if rgb8_is_grayscale(data.as_ref()) {
            let mut samples = Vec::with_capacity(data.len() / 3);
            let (pixels, _) = data.as_ref().as_chunks::<3>();
            for &[gray, _, _] in pixels {
                samples.push(gray);
            }
            Self::Gray {
                bit_depth: BitDepth::Eight,
                samples: Cow::Owned(samples),
            }
        } else {
            Self::Rgb8(data)
        }
    }

    pub fn infer_from_rgba8(data: impl Into<Cow<'a, [u8]>>) -> Self {
        let data = data.into();
        let bytes = data.as_ref();
        if rgba8_is_opaque_grayscale(bytes) {
            let mut samples = Vec::with_capacity(bytes.len() / 4);
            let (pixels, _) = bytes.as_chunks::<4>();
            for &[gray, _, _, _] in pixels {
                samples.push(gray);
            }
            return Self::Gray {
                bit_depth: BitDepth::Eight,
                samples: Cow::Owned(samples),
            };
        }
        if rgba8_is_grayscale(bytes) {
            let mut gray_alpha = Vec::with_capacity(bytes.len() / 2);
            let (pixels, _) = bytes.as_chunks::<4>();
            for &[gray, _, _, alpha] in pixels {
                gray_alpha.extend_from_slice(&[gray, alpha]);
            }
            return Self::GrayAlpha8(Cow::Owned(gray_alpha));
        }
        if rgba8_is_opaque(bytes) {
            let mut rgb = Vec::with_capacity(bytes.len() / 4 * 3);
            let (pixels, _) = bytes.as_chunks::<4>();
            for &[r, g, b, _] in pixels {
                rgb.extend_from_slice(&[r, g, b]);
            }
            return Self::Rgb8(Cow::Owned(rgb));
        }
        if let Some(indexed) = infer_indexed_from_rgba8(bytes) {
            return indexed;
        }
        Self::Rgba8(data)
    }

    pub fn pixel_count(&self) -> usize {
        match self {
            Self::Gray { samples, .. } => samples.len(),
            Self::Gray16(data) => data.len(),
            Self::GrayAlpha8(data) => data.len() / 2,
            Self::GrayAlpha16(data) => data.len() / 2,
            Self::Rgb8(data) => data.len() / 3,
            Self::Rgb16(data) => data.len() / 3,
            Self::Rgba8(data) => data.len() / 4,
            Self::Rgba16(data) => data.len() / 4,
            Self::Indexed { indices, .. } => indices.len(),
        }
    }

    pub fn color_mode(&self) -> ColorMode {
        match self {
            Self::Gray { .. } | Self::Gray16(_) => ColorMode::Grayscale,
            Self::GrayAlpha8(_) | Self::GrayAlpha16(_) => ColorMode::GrayscaleAlpha,
            Self::Rgb8(_) | Self::Rgb16(_) => ColorMode::Rgb,
            Self::Rgba8(_) | Self::Rgba16(_) => ColorMode::Rgba,
            Self::Indexed { .. } => ColorMode::Indexed,
        }
    }

    pub fn bit_depth(&self) -> BitDepth {
        match self {
            Self::Gray { bit_depth, .. } | Self::Indexed { bit_depth, .. } => *bit_depth,
            Self::Gray16(_) | Self::GrayAlpha16(_) | Self::Rgb16(_) | Self::Rgba16(_) => {
                BitDepth::Sixteen
            }
            Self::GrayAlpha8(_) | Self::Rgb8(_) | Self::Rgba8(_) => BitDepth::Eight,
        }
    }

    pub fn as_u8_storage(&self) -> Option<&[u8]> {
        match self {
            Self::Gray { samples, .. }
            | Self::GrayAlpha8(samples)
            | Self::Rgb8(samples)
            | Self::Rgba8(samples) => Some(samples.as_ref()),
            Self::Indexed {
                indices,
                palette: _,
                trns: _,
                ..
            } => Some(indices.as_ref()),
            _ => None,
        }
    }

    pub fn as_u16_storage(&self) -> Option<&[u16]> {
        match self {
            Self::Gray16(data)
            | Self::GrayAlpha16(data)
            | Self::Rgb16(data)
            | Self::Rgba16(data) => Some(data.as_ref()),
            _ => None,
        }
    }

    /// Creates a `'static` copy by cloning any borrowed data.
    ///
    /// Unlike [`Clone::clone`], which preserves the original lifetime, this
    /// method erases the lifetime parameter so the result can outlive the
    /// borrowed source. Use `clone()` when you need a copy with the same
    /// lifetime and `to_owned()` when you need a `Pixels<'static>`.
    pub fn to_owned(&self) -> Pixels<'static> {
        match self {
            Self::Gray { bit_depth, samples } => Pixels::Gray {
                bit_depth: *bit_depth,
                samples: Cow::Owned(samples.to_vec()),
            },
            Self::Gray16(data) => Pixels::Gray16(Cow::Owned(data.to_vec())),
            Self::GrayAlpha8(data) => Pixels::GrayAlpha8(Cow::Owned(data.to_vec())),
            Self::GrayAlpha16(data) => Pixels::GrayAlpha16(Cow::Owned(data.to_vec())),
            Self::Rgb8(data) => Pixels::Rgb8(Cow::Owned(data.to_vec())),
            Self::Rgb16(data) => Pixels::Rgb16(Cow::Owned(data.to_vec())),
            Self::Rgba8(data) => Pixels::Rgba8(Cow::Owned(data.to_vec())),
            Self::Rgba16(data) => Pixels::Rgba16(Cow::Owned(data.to_vec())),
            Self::Indexed {
                bit_depth,
                indices,
                palette,
                trns,
            } => Pixels::Indexed {
                bit_depth: *bit_depth,
                indices: Cow::Owned(indices.to_vec()),
                palette: Cow::Owned(palette.to_vec()),
                trns: trns.as_ref().map(|value| Cow::Owned(value.to_vec())),
            },
        }
    }

    pub fn into_owned(self) -> Pixels<'static> {
        match self {
            Self::Gray { bit_depth, samples } => Pixels::Gray {
                bit_depth,
                samples: Cow::Owned(samples.into_owned()),
            },
            Self::Gray16(data) => Pixels::Gray16(Cow::Owned(data.into_owned())),
            Self::GrayAlpha8(data) => Pixels::GrayAlpha8(Cow::Owned(data.into_owned())),
            Self::GrayAlpha16(data) => Pixels::GrayAlpha16(Cow::Owned(data.into_owned())),
            Self::Rgb8(data) => Pixels::Rgb8(Cow::Owned(data.into_owned())),
            Self::Rgb16(data) => Pixels::Rgb16(Cow::Owned(data.into_owned())),
            Self::Rgba8(data) => Pixels::Rgba8(Cow::Owned(data.into_owned())),
            Self::Rgba16(data) => Pixels::Rgba16(Cow::Owned(data.into_owned())),
            Self::Indexed {
                bit_depth,
                indices,
                palette,
                trns,
            } => Pixels::Indexed {
                bit_depth,
                indices: Cow::Owned(indices.into_owned()),
                palette: Cow::Owned(palette.into_owned()),
                trns: trns.map(|value| Cow::Owned(value.into_owned())),
            },
        }
    }

    pub fn to_gray8(&self) -> Pixels<'static> {
        Pixels::Gray {
            bit_depth: BitDepth::Eight,
            samples: Cow::Owned(self.to_gray8_vec()),
        }
    }

    pub fn to_gray16(&self) -> Pixels<'static> {
        Pixels::Gray16(Cow::Owned(self.to_gray16_vec()))
    }

    pub fn to_rgb8(&self) -> Pixels<'static> {
        Pixels::Rgb8(Cow::Owned(self.to_rgb8_vec()))
    }

    pub fn to_rgb16(&self) -> Pixels<'static> {
        Pixels::Rgb16(Cow::Owned(self.to_rgb16_vec()))
    }

    pub fn to_rgba8(&self) -> Pixels<'static> {
        Pixels::Rgba8(Cow::Owned(self.to_rgba8_vec()))
    }

    pub fn to_rgba16(&self) -> Pixels<'static> {
        Pixels::Rgba16(Cow::Owned(self.to_rgba16_vec()))
    }

    pub(crate) fn to_gray8_vec(&self) -> Vec<u8> {
        match self {
            Self::Gray { bit_depth, samples } => {
                if *bit_depth == BitDepth::Eight {
                    samples.to_vec()
                } else {
                    samples
                        .iter()
                        .map(|&sample| scale_sample_to_u8(u16::from(sample), bit_depth.as_u8()))
                        .collect()
                }
            }
            Self::Gray16(data) => data.iter().copied().map(downsample_u16).collect(),
            Self::GrayAlpha8(data) => {
                let (pairs, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                pairs.iter().map(|[gray, _]| *gray).collect()
            }
            Self::GrayAlpha16(data) => {
                let (pairs, remainder) = data.as_chunks::<2>();
                debug_assert!(remainder.is_empty());
                pairs
                    .iter()
                    .map(|[gray, _]| downsample_u16(*gray))
                    .collect()
            }
            Self::Rgb8(_)
            | Self::Rgb16(_)
            | Self::Rgba8(_)
            | Self::Rgba16(_)
            | Self::Indexed { .. } => {
                let rgba = self.to_rgba8_vec();
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels
                    .iter()
                    .map(|[r, g, b, _]| rgb_to_gray8(*r, *g, *b))
                    .collect()
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
                pixels
                    .iter()
                    .map(|[r, g, b, _]| rgb_to_gray16(*r, *g, *b))
                    .collect()
            }
            _ => self
                .to_gray8_vec()
                .into_iter()
                .map(upscale_u8_to_u16)
                .collect(),
        }
    }

    pub(crate) fn to_rgb8_vec(&self) -> Vec<u8> {
        match self {
            Self::Rgb8(data) => data.to_vec(),
            Self::Rgb16(data) => data.iter().copied().map(downsample_u16).collect(),
            Self::Gray { .. } | Self::Gray16(_) | Self::GrayAlpha8(_) | Self::GrayAlpha16(_) => {
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
                pixels
                    .iter()
                    .flat_map(|[r, g, b, _]| [*r, *g, *b])
                    .collect()
            }
            Self::Rgba16(data) => {
                let (pixels, remainder) = data.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels
                    .iter()
                    .flat_map(|[r, g, b, _]| {
                        [downsample_u16(*r), downsample_u16(*g), downsample_u16(*b)]
                    })
                    .collect()
            }
            Self::Indexed { .. } => {
                let rgba = self.to_rgba8_vec();
                let (pixels, remainder) = rgba.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels
                    .iter()
                    .flat_map(|[r, g, b, _]| [*r, *g, *b])
                    .collect()
            }
        }
    }

    pub(crate) fn to_rgb16_vec(&self) -> Vec<u16> {
        match self {
            Self::Rgb16(data) => data.to_vec(),
            Self::Rgba16(data) => {
                let (pixels, remainder) = data.as_chunks::<4>();
                debug_assert!(remainder.is_empty());
                pixels
                    .iter()
                    .flat_map(|[r, g, b, _]| [*r, *g, *b])
                    .collect()
            }
            _ => self
                .to_rgb8_vec()
                .into_iter()
                .map(upscale_u8_to_u16)
                .collect(),
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
                    rgba.extend_from_slice(&[
                        downsample_u16(r),
                        downsample_u16(g),
                        downsample_u16(b),
                        255,
                    ]);
                }
                rgba
            }
            Self::Gray { .. } | Self::Gray16(_) => {
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
                    rgba.extend_from_slice(&[
                        downsample_u16(gray),
                        downsample_u16(gray),
                        downsample_u16(gray),
                        downsample_u16(alpha),
                    ]);
                }
                rgba
            }
            Self::Indexed { .. } => indexed_to_rgba8(self),
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
            _ => self
                .to_rgba8_vec()
                .into_iter()
                .map(upscale_u8_to_u16)
                .collect(),
        }
    }
}

fn rgb8_is_grayscale(data: &[u8]) -> bool {
    let (pixels, _) = data.as_chunks::<3>();
    pixels.iter().all(|[r, g, b]| r == g && g == b)
}

fn rgba8_is_grayscale(data: &[u8]) -> bool {
    let (pixels, _) = data.as_chunks::<4>();
    pixels.iter().all(|[r, g, b, _]| r == g && g == b)
}

fn rgba8_is_opaque(data: &[u8]) -> bool {
    let (pixels, _) = data.as_chunks::<4>();
    pixels.iter().all(|[_, _, _, alpha]| *alpha == 255)
}

fn rgba8_is_opaque_grayscale(data: &[u8]) -> bool {
    rgba8_is_grayscale(data) && rgba8_is_opaque(data)
}

fn infer_indexed_from_rgba8(data: &[u8]) -> Option<Pixels<'static>> {
    let (pixels, _) = data.as_chunks::<4>();
    let mut map = BTreeMap::<[u8; 4], u8>::new();
    let mut palette = Vec::new();
    let mut trns = Vec::new();
    let mut indices = Vec::with_capacity(pixels.len());
    for &[r, g, b, a] in pixels {
        let color = [r, g, b, a];
        let index = if let Some(&index) = map.get(&color) {
            index
        } else {
            let next = u8::try_from(map.len()).ok()?;
            if map.len() == 256 {
                return None;
            }
            map.insert(color, next);
            palette.extend_from_slice(&[r, g, b]);
            trns.push(a);
            next
        };
        indices.push(index);
    }
    Some(Pixels::Indexed {
        bit_depth: BitDepth::Eight,
        indices: Cow::Owned(indices),
        palette: Cow::Owned(palette),
        trns: Some(Cow::Owned(trns)),
    })
}

pub(crate) fn validate_pixels(pixels: &Pixels<'_>) -> Result<()> {
    match pixels {
        Pixels::Gray { bit_depth, samples } => {
            validate_gray_bit_depth(*bit_depth)?;
            if *bit_depth == BitDepth::Sixteen {
                return Err(Error::InvalidData(
                    "Gray uses 8-bit storage; use Gray16 for 16-bit grayscale pixels".into(),
                ));
            }
            validate_sample_range(samples, bit_depth.as_u8(), "Gray")
        }
        Pixels::Gray16(_)
        | Pixels::GrayAlpha8(_)
        | Pixels::GrayAlpha16(_)
        | Pixels::Rgb8(_)
        | Pixels::Rgb16(_)
        | Pixels::Rgba8(_)
        | Pixels::Rgba16(_) => Ok(()),
        Pixels::Indexed {
            bit_depth,
            indices,
            palette,
            trns,
        } => {
            validate_indexed_bit_depth(*bit_depth)?;
            validate_indexed_pixels(indices, palette, trns.as_deref(), bit_depth.as_u8())
        }
    }
}

fn validate_gray_bit_depth(bit_depth: BitDepth) -> Result<()> {
    match bit_depth {
        BitDepth::One | BitDepth::Two | BitDepth::Four | BitDepth::Eight => Ok(()),
        BitDepth::Sixteen => Err(Error::InvalidData(
            "Gray uses unpacked u8 samples and does not support 16-bit depth".into(),
        )),
    }
}

fn validate_indexed_bit_depth(bit_depth: BitDepth) -> Result<()> {
    match bit_depth {
        BitDepth::One | BitDepth::Two | BitDepth::Four | BitDepth::Eight => Ok(()),
        BitDepth::Sixteen => Err(Error::InvalidData(
            "indexed pixels do not support 16-bit depth".into(),
        )),
    }
}

fn validate_sample_range(samples: &[u8], bit_depth: u8, name: &str) -> Result<()> {
    let max = (1u16 << bit_depth) - 1;
    if samples.iter().all(|&sample| u16::from(sample) <= max) {
        Ok(())
    } else {
        Err(Error::InvalidData(
            format!("{name} contains a sample that does not fit in {bit_depth} bits").into(),
        ))
    }
}

fn validate_indexed_pixels(
    indices: &[u8],
    palette: &[u8],
    trns: Option<&[u8]>,
    bit_depth: u8,
) -> Result<()> {
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
    if let Some(trns) = trns
        && trns.len() > palette_len
    {
        return Err(Error::InvalidData(
            "indexed transparency table is longer than the palette".into(),
        ));
    }
    let capacity = 1usize << bit_depth;
    if palette_len > capacity {
        return Err(Error::InvalidData(
            format!(
                "palette of size {} does not fit in {}-bit indexed pixels",
                palette_len, bit_depth
            )
            .into(),
        ));
    }
    if indices
        .iter()
        .all(|&index| usize::from(index) < palette_len)
    {
        Ok(())
    } else {
        Err(Error::InvalidData(
            "indexed pixel buffer contains an out-of-range palette index".into(),
        ))
    }
}

pub(crate) fn indexed_to_rgba8(pixels: &Pixels<'_>) -> Vec<u8> {
    let (indices, palette, trns) = match pixels {
        Pixels::Indexed {
            indices,
            palette,
            trns,
            ..
        } => (indices.as_ref(), palette.as_ref(), trns.as_deref()),
        _ => unreachable!("bug: indexed_to_rgba8 requires indexed pixels"),
    };
    let mut rgba = Vec::with_capacity(indices.len() * 4);
    for &index in indices {
        let rgb_index = usize::from(index) * 3;
        let rgb = &palette[rgb_index..rgb_index + 3];
        let alpha = trns
            .and_then(|table| table.get(usize::from(index)))
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
