use alloc::vec::Vec;

use crate::png_types::{Error, PixelFormat, Result};

pub(crate) fn reformat(
    src_fmt: &PixelFormat,
    src: &[u8],
    dst_fmt: &PixelFormat,
) -> Result<Vec<u8>> {
    // Identity: just clone.
    if src_fmt == dst_fmt {
        return Ok(src.to_vec());
    }
    // Route through RGBA8 or RGBA16Be intermediate for simplicity.
    match dst_fmt {
        PixelFormat::Rgba8 => to_rgba8(src_fmt, src),
        PixelFormat::Rgba16Be => to_rgba16be(src_fmt, src),
        PixelFormat::Rgb8 => to_rgb8(src_fmt, src),
        PixelFormat::Rgb16Be => to_rgb16be(src_fmt, src),
        PixelFormat::Gray1
        | PixelFormat::Gray2
        | PixelFormat::Gray4
        | PixelFormat::Gray8
        | PixelFormat::Gray16Be => to_gray(src_fmt, src, dst_fmt.bit_depth()),
        PixelFormat::GrayAlpha8 => to_grayalpha8(src_fmt, src),
        PixelFormat::GrayAlpha16Be => to_grayalpha16be(src_fmt, src),
        PixelFormat::Indexed1 { .. }
        | PixelFormat::Indexed2 { .. }
        | PixelFormat::Indexed4 { .. }
        | PixelFormat::Indexed8 { .. } => Err(Error::Unsupported(
            "reformatting to indexed format is not supported".into(),
        )),
    }
}

// ── Conversion to RGBA8 ─────────────────────────────────────────────────

fn to_rgba8(src_fmt: &PixelFormat, src: &[u8]) -> Result<Vec<u8>> {
    match src_fmt {
        PixelFormat::Rgba8 => Ok(src.to_vec()),
        PixelFormat::Rgba16Be => {
            let (chunks, remainder) = src.as_chunks::<8>();
            debug_assert!(remainder.is_empty());
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for &[r_hi, _r_lo, g_hi, _g_lo, b_hi, _b_lo, a_hi, _a_lo] in chunks {
                out.extend_from_slice(&[r_hi, g_hi, b_hi, a_hi]);
            }
            Ok(out)
        }
        PixelFormat::Rgb8 => {
            let (chunks, remainder) = src.as_chunks::<3>();
            debug_assert!(remainder.is_empty());
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for &[r, g, b] in chunks {
                out.extend_from_slice(&[r, g, b, 255]);
            }
            Ok(out)
        }
        PixelFormat::Rgb16Be => {
            let (chunks, remainder) = src.as_chunks::<6>();
            debug_assert!(remainder.is_empty());
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for &[r, _, g, _, b, _, ..] in chunks {
                out.extend_from_slice(&[r, g, b, 255]);
            }
            Ok(out)
        }
        PixelFormat::Gray16Be => {
            let (chunks, _) = src.as_chunks::<2>();
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for &[hi, _lo] in chunks {
                out.extend_from_slice(&[hi, hi, hi, 255]);
            }
            Ok(out)
        }
        PixelFormat::Gray1 | PixelFormat::Gray2 | PixelFormat::Gray4 | PixelFormat::Gray8 => {
            let bit_depth = src_fmt.bit_depth();
            let mut out = Vec::with_capacity(src.len() * 4);
            for &sample in src {
                let gray = scale_sample_to_u8(u16::from(sample), bit_depth);
                out.extend_from_slice(&[gray, gray, gray, 255]);
            }
            Ok(out)
        }
        PixelFormat::GrayAlpha8 => {
            let (chunks, remainder) = src.as_chunks::<2>();
            debug_assert!(remainder.is_empty());
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for &[gray, alpha] in chunks {
                out.extend_from_slice(&[gray, gray, gray, alpha]);
            }
            Ok(out)
        }
        PixelFormat::GrayAlpha16Be => {
            let (chunks, remainder) = src.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for &[g_hi, _g_lo, a_hi, _a_lo] in chunks {
                out.extend_from_slice(&[g_hi, g_hi, g_hi, a_hi]);
            }
            Ok(out)
        }
        PixelFormat::Indexed1 { palette, trns, .. }
        | PixelFormat::Indexed2 { palette, trns, .. }
        | PixelFormat::Indexed4 { palette, trns, .. }
        | PixelFormat::Indexed8 { palette, trns, .. } => {
            let mut out = Vec::with_capacity(src.len() * 4);
            for &index in src {
                let rgb_offset = usize::from(index) * 3;
                if rgb_offset + 3 > palette.len() {
                    return Err(Error::InvalidData("palette index out of range".into()));
                }
                let r = palette[rgb_offset];
                let g = palette[rgb_offset + 1];
                let b = palette[rgb_offset + 2];
                let a = trns
                    .as_ref()
                    .and_then(|t| t.get(usize::from(index)))
                    .copied()
                    .unwrap_or(255);
                out.extend_from_slice(&[r, g, b, a]);
            }
            Ok(out)
        }
    }
}

// ── Conversion to RGBA16Be ──────────────────────────────────────────────

fn to_rgba16be(src_fmt: &PixelFormat, src: &[u8]) -> Result<Vec<u8>> {
    match src_fmt {
        PixelFormat::Rgba16Be => Ok(src.to_vec()),
        PixelFormat::Rgba8 => {
            let (chunks, _) = src.as_chunks::<4>();
            let mut out = Vec::with_capacity(chunks.len() * 8);
            for &[r, g, b, a] in chunks {
                for sample in [r, g, b, a] {
                    out.extend_from_slice(&upscale_u8_to_u16(sample).to_be_bytes());
                }
            }
            Ok(out)
        }
        PixelFormat::Rgb16Be => {
            let (chunks, _) = src.as_chunks::<6>();
            let mut out = Vec::with_capacity(chunks.len() * 8);
            for chunk in chunks {
                out.extend_from_slice(chunk);
                out.extend_from_slice(&u16::MAX.to_be_bytes());
            }
            Ok(out)
        }
        PixelFormat::Gray16Be => {
            let (chunks, _) = src.as_chunks::<2>();
            let mut out = Vec::with_capacity(chunks.len() * 8);
            for chunk in chunks {
                out.extend_from_slice(chunk); // r
                out.extend_from_slice(chunk); // g
                out.extend_from_slice(chunk); // b
                out.extend_from_slice(&u16::MAX.to_be_bytes()); // a
            }
            Ok(out)
        }
        PixelFormat::GrayAlpha16Be => {
            let (chunks, _) = src.as_chunks::<4>();
            let mut out = Vec::with_capacity(chunks.len() * 8);
            for &[g_hi, g_lo, a_hi, a_lo] in chunks {
                out.extend_from_slice(&[g_hi, g_lo, g_hi, g_lo, g_hi, g_lo, a_hi, a_lo]);
            }
            Ok(out)
        }
        _ => {
            // For 8-bit types, go through RGBA8 → upscale.
            let rgba8 = to_rgba8(src_fmt, src)?;
            to_rgba16be(&PixelFormat::Rgba8, &rgba8)
        }
    }
}

// ── Conversion to RGB8 ──────────────────────────────────────────────────

fn to_rgb8(src_fmt: &PixelFormat, src: &[u8]) -> Result<Vec<u8>> {
    match src_fmt {
        PixelFormat::Rgb8 => Ok(src.to_vec()),
        PixelFormat::Rgba8 => {
            let (chunks, _) = src.as_chunks::<4>();
            Ok(chunks
                .iter()
                .flat_map(|[r, g, b, _]| [*r, *g, *b])
                .collect())
        }
        PixelFormat::Rgb16Be => {
            let (chunks, _) = src.as_chunks::<6>();
            Ok(chunks
                .iter()
                .flat_map(|[r, _, g, _, b, _, ..]| [*r, *g, *b])
                .collect())
        }
        _ => {
            let rgba8 = to_rgba8(src_fmt, src)?;
            to_rgb8(&PixelFormat::Rgba8, &rgba8)
        }
    }
}

// ── Conversion to RGB16Be ───────────────────────────────────────────────

fn to_rgb16be(src_fmt: &PixelFormat, src: &[u8]) -> Result<Vec<u8>> {
    match src_fmt {
        PixelFormat::Rgb16Be => Ok(src.to_vec()),
        PixelFormat::Rgba16Be => {
            let (chunks, _) = src.as_chunks::<8>();
            Ok(chunks
                .iter()
                .flat_map(|c| [c[0], c[1], c[2], c[3], c[4], c[5]])
                .collect())
        }
        _ => {
            let rgba16 = to_rgba16be(src_fmt, src)?;
            to_rgb16be(&PixelFormat::Rgba16Be, &rgba16)
        }
    }
}

// ── Conversion to Gray ──────────────────────────────────────────────────

fn to_gray(src_fmt: &PixelFormat, src: &[u8], dst_depth: u8) -> Result<Vec<u8>> {
    let src_depth = src_fmt.bit_depth();
    let src_is_gray = matches!(
        src_fmt,
        PixelFormat::Gray1
            | PixelFormat::Gray2
            | PixelFormat::Gray4
            | PixelFormat::Gray8
            | PixelFormat::Gray16Be
    );
    if src_is_gray && src_depth == dst_depth {
        return Ok(src.to_vec());
    }
    if src_is_gray && dst_depth == 8 {
        if src_depth == 16 {
            let (chunks, _) = src.as_chunks::<2>();
            return Ok(chunks.iter().map(|[hi, _lo]| *hi).collect());
        } else {
            return Ok(src
                .iter()
                .map(|&s| scale_sample_to_u8(u16::from(s), src_depth))
                .collect());
        }
    }
    if dst_depth == 8 {
        let rgba8 = to_rgba8(src_fmt, src)?;
        let (chunks, _) = rgba8.as_chunks::<4>();
        return Ok(chunks
            .iter()
            .map(|[r, g, b, _]| rgb_to_gray8(*r, *g, *b))
            .collect());
    }
    if dst_depth == 16 {
        let rgba16 = to_rgba16be(src_fmt, src)?;
        let (chunks, _) = rgba16.as_chunks::<8>();
        let mut out = Vec::with_capacity(chunks.len() * 2);
        for chunk in chunks {
            let r = u16::from_be_bytes([chunk[0], chunk[1]]);
            let g = u16::from_be_bytes([chunk[2], chunk[3]]);
            let b = u16::from_be_bytes([chunk[4], chunk[5]]);
            out.extend_from_slice(&rgb_to_gray16(r, g, b).to_be_bytes());
        }
        return Ok(out);
    }
    Err(Error::Unsupported(
        "unsupported gray bit depth conversion".into(),
    ))
}

// ── Conversion to GrayAlpha8 ────────────────────────────────────────────

fn to_grayalpha8(src_fmt: &PixelFormat, src: &[u8]) -> Result<Vec<u8>> {
    match src_fmt {
        PixelFormat::GrayAlpha8 => Ok(src.to_vec()),
        _ => {
            let rgba8 = to_rgba8(src_fmt, src)?;
            let (chunks, _) = rgba8.as_chunks::<4>();
            Ok(chunks
                .iter()
                .flat_map(|[r, g, b, a]| [rgb_to_gray8(*r, *g, *b), *a])
                .collect())
        }
    }
}

// ── Conversion to GrayAlpha16Be ─────────────────────────────────────────

fn to_grayalpha16be(src_fmt: &PixelFormat, src: &[u8]) -> Result<Vec<u8>> {
    match src_fmt {
        PixelFormat::GrayAlpha16Be => Ok(src.to_vec()),
        _ => {
            let rgba16 = to_rgba16be(src_fmt, src)?;
            let (chunks, _) = rgba16.as_chunks::<8>();
            let mut out = Vec::with_capacity(chunks.len() * 4);
            for chunk in chunks {
                let r = u16::from_be_bytes([chunk[0], chunk[1]]);
                let g = u16::from_be_bytes([chunk[2], chunk[3]]);
                let b = u16::from_be_bytes([chunk[4], chunk[5]]);
                let gray = rgb_to_gray16(r, g, b);
                out.extend_from_slice(&gray.to_be_bytes());
                out.extend_from_slice(&[chunk[6], chunk[7]]); // alpha
            }
            Ok(out)
        }
    }
}

// ── Helper functions ────────────────────────────────────────────────────

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

fn rgb_to_gray8(r: u8, g: u8, b: u8) -> u8 {
    (((u32::from(r) * 299) + (u32::from(g) * 587) + (u32::from(b) * 114) + 500) / 1000) as u8
}

fn rgb_to_gray16(r: u16, g: u16, b: u16) -> u16 {
    (((u32::from(r) * 299) + (u32::from(g) * 587) + (u32::from(b) * 114) + 500) / 1000) as u16
}

// ── Validation ──────────────────────────────────────────────────────────

pub(crate) fn validate_format_and_data(
    format: &PixelFormat,
    data: &[u8],
    width: u32,
    height: u32,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Err(Error::InvalidData(
            "image dimensions must be non-zero".into(),
        ));
    }
    let expected = format.data_len(width, height);
    if data.len() != expected {
        return Err(Error::InvalidData(
            "image size does not match pixel buffer length".into(),
        ));
    }
    match format {
        PixelFormat::Gray1 | PixelFormat::Gray2 | PixelFormat::Gray4 => {
            validate_sample_range(data, format.bit_depth())?;
        }
        PixelFormat::Indexed1 { palette, trns, .. } => {
            validate_indexed_format(1, data, palette, trns.as_deref())?;
        }
        PixelFormat::Indexed2 { palette, trns, .. } => {
            validate_indexed_format(2, data, palette, trns.as_deref())?;
        }
        PixelFormat::Indexed4 { palette, trns, .. } => {
            validate_indexed_format(4, data, palette, trns.as_deref())?;
        }
        PixelFormat::Indexed8 { palette, trns, .. } => {
            validate_indexed_format(8, data, palette, trns.as_deref())?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_sample_range(samples: &[u8], bit_depth: u8) -> Result<()> {
    let max = (1u16 << bit_depth) - 1;
    if samples.iter().all(|&s| u16::from(s) <= max) {
        Ok(())
    } else {
        Err(Error::InvalidData(
            alloc::format!("sample does not fit in {bit_depth} bits").into(),
        ))
    }
}

fn validate_indexed_format(
    bit_depth: u8,
    indices: &[u8],
    palette: &[u8],
    trns: Option<&[u8]>,
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
            alloc::format!(
                "palette of size {} does not fit in {}-bit indexed pixels",
                palette_len,
                bit_depth
            )
            .into(),
        ));
    }
    if !indices
        .iter()
        .all(|&index| usize::from(index) < palette_len)
    {
        return Err(Error::InvalidData(
            "indexed pixel buffer contains an out-of-range palette index".into(),
        ));
    }
    Ok(())
}
