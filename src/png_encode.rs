use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::chunk::IhdrChunk;
use crate::png_pixels::PngPixels;
use crate::png_types::{Error, PngBitDepth, PngColorMode, PngEncoding, Result};

use crate::png::{ADAM7_PASSES, adam7_axis_size};

#[derive(Debug)]
pub(crate) struct EncodedImage {
    pub(crate) bit_depth: u8,
    pub(crate) color_type: u8,
    pub(crate) interlace_method: u8,
    pub(crate) filtered_data: Vec<u8>,
    pub(crate) palette: Option<Vec<[u8; 3]>>,
    pub(crate) trns: Option<Vec<u8>>,
}

impl EncodedImage {
    pub(crate) fn from_pixels(
        width: u32,
        height: u32,
        pixels: &PngPixels<'_>,
        encoding: PngEncoding,
    ) -> Result<Self> {
        // Fast path: Indexed pixels -> Indexed encoding with matching bit depth.
        if encoding.color_mode == PngColorMode::Indexed
            && pixels.color_mode() == PngColorMode::Indexed
            && pixels.bit_depth() == encoding.bit_depth
            && let Some(result) = Self::from_indexed_direct(width, height, pixels, encoding)
        {
            return result;
        }
        if encoding.bit_depth == PngBitDepth::Sixteen {
            let rgba16 = pixels.to_rgba16_vec();
            return Self::from_rgba16(width, height, &rgba16, encoding);
        }
        let rgba8 = pixels.to_rgba8_vec();
        Self::from_rgba(width, height, &rgba8, encoding)
    }

    /// Fast path for Indexed pixels that already have palette/indices.
    fn from_indexed_direct(
        width: u32,
        height: u32,
        pixels: &PngPixels<'_>,
        encoding: PngEncoding,
    ) -> Option<Result<Self>> {
        let indices = pixels.indices()?;
        let flat_palette = pixels.palette()?;
        let trns = pixels.trns();

        if flat_palette.is_empty() || !flat_palette.len().is_multiple_of(3) {
            return None;
        }
        let palette_len = flat_palette.len() / 3;
        let bit_depth = encoding.bit_depth.as_u8();
        let capacity = 1usize << bit_depth;
        if palette_len > capacity {
            return None;
        }

        let palette: Vec<[u8; 3]> = flat_palette
            .chunks_exact(3)
            .map(|chunk| [chunk[0], chunk[1], chunk[2]])
            .collect();
        let trns_vec = trns.map(|t| t.to_vec());

        let bpp = 1usize; // Indexed always 1 byte per pixel for filter purposes.
        let interlace_method = u8::from(encoding.interlaced);

        let filtered_data = if encoding.interlaced {
            build_indexed_adam7_filtered_data(width, height, indices, bit_depth, bpp)
        } else {
            build_indexed_filtered_data(width, height, indices, bit_depth, bpp)
        };

        Some(Ok(Self {
            bit_depth,
            color_type: IhdrChunk::COLOR_TYPE_INDEXED,
            interlace_method,
            filtered_data,
            palette: Some(palette),
            trns: trns_vec,
        }))
    }

    pub(crate) fn from_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
        encoding: PngEncoding,
    ) -> Result<Self> {
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

    pub(crate) fn from_rgba16(
        width: u32,
        height: u32,
        rgba: &[u16],
        encoding: PngEncoding,
    ) -> Result<Self> {
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
    color_map: Option<BTreeMap<[u8; 4], u8>>,
}

impl EncodingTarget {
    fn filter_bpp(&self) -> usize {
        match self.pixel_kind {
            EncodedPixelKind::GrayscalePacked | EncodedPixelKind::Indexed => 1,
            EncodedPixelKind::Grayscale8 => 1,
            EncodedPixelKind::GrayscaleAlpha8 => 2,
            EncodedPixelKind::Rgb8 => 3,
            EncodedPixelKind::Rgba8 => 4,
        }
    }
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

impl EncodingTarget16 {
    fn filter_bpp(&self) -> usize {
        match self.pixel_kind {
            EncodedPixelKind16::Grayscale16 => 2,
            EncodedPixelKind16::GrayscaleAlpha16 => 4,
            EncodedPixelKind16::Rgb16 => 6,
            EncodedPixelKind16::Rgba16 => 8,
        }
    }
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
                    color_map: None,
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
                    color_map: None,
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
                    color_map: None,
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
                    color_map: None,
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
                    color_map: Some(indexed.color_map),
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
    color_map: BTreeMap<[u8; 4], u8>,
}

fn build_filtered_data(
    width: u32,
    height: u32,
    pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<Vec<u8>> {
    let bpp = target.filter_bpp();
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    for row in 0..height as usize {
        raw_row.clear();
        let row_pixels = &pixels[row * width as usize..(row + 1) * width as usize];
        encode_row_into(&mut raw_row, row_pixels, target)?;
        let prev = if row == 0 { None } else { Some(prev_row.as_slice()) };
        write_filtered_row(&mut filtered, &raw_row, prev, bpp);
        prev_row.clear();
        prev_row.extend_from_slice(&raw_row);
    }
    Ok(filtered)
}

fn build_adam7_filtered_data(
    width: u32,
    height: u32,
    pixels: &[[u8; 4]],
    target: &EncodingTarget,
) -> Result<Vec<u8>> {
    let bpp = target.filter_bpp();
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    let mut pass_row_pixels = Vec::new();
    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        prev_row.clear();
        for pass_y in 0..pass_height as usize {
            raw_row.clear();
            pass_row_pixels.clear();
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            pass_row_pixels.extend((0..pass_width as usize).map(|pass_x| {
                let x = pass.x_start as usize + pass_x * pass.x_step as usize;
                pixels[y * width as usize + x]
            }));
            encode_row_into(&mut raw_row, &pass_row_pixels, target)?;
            let prev = if pass_y == 0 { None } else { Some(prev_row.as_slice()) };
            write_filtered_row(&mut filtered, &raw_row, prev, bpp);
            prev_row.clear();
            prev_row.extend_from_slice(&raw_row);
        }
    }
    Ok(filtered)
}

fn build_indexed_filtered_data(
    width: u32,
    height: u32,
    indices: &[u8],
    bit_depth: u8,
    bpp: usize,
) -> Vec<u8> {
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    for row in 0..height as usize {
        raw_row.clear();
        let row_indices = &indices[row * width as usize..(row + 1) * width as usize];
        pack_samples_to(&mut raw_row, row_indices, bit_depth);
        let prev = if row == 0 {
            None
        } else {
            Some(prev_row.as_slice())
        };
        write_filtered_row(&mut filtered, &raw_row, prev, bpp);
        prev_row.clear();
        prev_row.extend_from_slice(&raw_row);
    }
    filtered
}

fn build_indexed_adam7_filtered_data(
    width: u32,
    height: u32,
    indices: &[u8],
    bit_depth: u8,
    bpp: usize,
) -> Vec<u8> {
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    let mut pass_row_indices = Vec::new();
    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        prev_row.clear();
        for pass_y in 0..pass_height as usize {
            raw_row.clear();
            pass_row_indices.clear();
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            pass_row_indices.extend((0..pass_width as usize).map(|pass_x| {
                let x = pass.x_start as usize + pass_x * pass.x_step as usize;
                indices[y * width as usize + x]
            }));
            pack_samples_to(&mut raw_row, &pass_row_indices, bit_depth);
            let prev = if pass_y == 0 {
                None
            } else {
                Some(prev_row.as_slice())
            };
            write_filtered_row(&mut filtered, &raw_row, prev, bpp);
            prev_row.clear();
            prev_row.extend_from_slice(&raw_row);
        }
    }
    filtered
}

fn build_filtered_data16(
    width: u32,
    height: u32,
    pixels: &[[u16; 4]],
    target: &EncodingTarget16,
) -> Result<Vec<u8>> {
    let bpp = target.filter_bpp();
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    for row in 0..height as usize {
        raw_row.clear();
        let row_pixels = &pixels[row * width as usize..(row + 1) * width as usize];
        encode_row16_into(&mut raw_row, row_pixels, target);
        let prev = if row == 0 { None } else { Some(prev_row.as_slice()) };
        write_filtered_row(&mut filtered, &raw_row, prev, bpp);
        prev_row.clear();
        prev_row.extend_from_slice(&raw_row);
    }
    Ok(filtered)
}

fn build_adam7_filtered_data16(
    width: u32,
    height: u32,
    pixels: &[[u16; 4]],
    target: &EncodingTarget16,
) -> Result<Vec<u8>> {
    let bpp = target.filter_bpp();
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    let mut pass_row_pixels = Vec::new();
    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        prev_row.clear();
        for pass_y in 0..pass_height as usize {
            raw_row.clear();
            pass_row_pixels.clear();
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            pass_row_pixels.extend((0..pass_width as usize).map(|pass_x| {
                let x = pass.x_start as usize + pass_x * pass.x_step as usize;
                pixels[y * width as usize + x]
            }));
            encode_row16_into(&mut raw_row, &pass_row_pixels, target);
            let prev = if pass_y == 0 { None } else { Some(prev_row.as_slice()) };
            write_filtered_row(&mut filtered, &raw_row, prev, bpp);
            prev_row.clear();
            prev_row.extend_from_slice(&raw_row);
        }
    }
    Ok(filtered)
}

/// Select the best filter from None (0), Sub (1), Up (2), and Paeth (4)
/// using minimum-sum-of-absolutes heuristic.
fn write_filtered_row(out: &mut Vec<u8>, raw: &[u8], prev: Option<&[u8]>, bpp: usize) {
    let abs_cost = |b: u8| u32::from((b as i8).unsigned_abs());

    // Filter 0: None
    let none_cost: u32 = raw.iter().map(|&b| abs_cost(b)).sum();

    // Filter 1: Sub
    let sub_cost: u32 = raw
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let left = if i >= bpp { raw[i - bpp] } else { 0 };
            abs_cost(b.wrapping_sub(left))
        })
        .sum();

    // Filter 2: Up
    let up_cost: u32 = raw
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let up = prev.map_or(0, |p| p[i]);
            abs_cost(b.wrapping_sub(up))
        })
        .sum();

    // Filter 4: Paeth
    let paeth_cost: u32 = raw
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let left = if i >= bpp { raw[i - bpp] } else { 0 };
            let up = prev.map_or(0, |p| p[i]);
            let up_left = if i >= bpp {
                prev.map_or(0, |p| p[i - bpp])
            } else {
                0
            };
            abs_cost(b.wrapping_sub(paeth_predictor(left, up, up_left)))
        })
        .sum();

    let (best_filter, best_cost) = [(0u8, none_cost), (1, sub_cost), (2, up_cost), (4, paeth_cost)]
        .into_iter()
        .min_by_key(|&(_, cost)| cost)
        .unwrap();

    out.reserve(1 + raw.len());
    out.push(best_filter);

    if best_cost == none_cost && best_filter == 0 {
        out.extend_from_slice(raw);
    } else {
        for (i, &b) in raw.iter().enumerate() {
            let left = if i >= bpp { raw[i - bpp] } else { 0 };
            let up = prev.map_or(0, |p| p[i]);
            let filtered_byte = match best_filter {
                1 => b.wrapping_sub(left),
                2 => b.wrapping_sub(up),
                4 => {
                    let up_left = if i >= bpp {
                        prev.map_or(0, |p| p[i - bpp])
                    } else {
                        0
                    };
                    b.wrapping_sub(paeth_predictor(left, up, up_left))
                }
                _ => b,
            };
            out.push(filtered_byte);
        }
    }
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
            let color_map = target
                .color_map
                .as_ref()
                .expect("bug: indexed encoding target must include a color map");
            if target.bit_depth == 8 {
                for pixel in row_pixels {
                    let index = color_map.get(pixel).copied().ok_or_else(|| {
                        Error::InvalidData("pixel missing from indexed palette".into())
                    })?;
                    out.push(index);
                }
            } else {
                let mut acc = 0u16;
                let mut bits = 0usize;
                let bd = usize::from(target.bit_depth);
                for pixel in row_pixels {
                    let index = color_map.get(pixel).copied().ok_or_else(|| {
                        Error::InvalidData("pixel missing from indexed palette".into())
                    })?;
                    acc = (acc << bd) | u16::from(index);
                    bits += bd;
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
            alloc::format!(
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
            alloc::format!(
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
            alloc::format!(
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
    let color_map = map.into_iter().map(|(k, v)| (k, v as u8)).collect();
    Some(IndexedAnalysis {
        palette,
        trns,
        color_map,
    })
}
