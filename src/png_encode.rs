use alloc::vec::Vec;

use crate::chunk::IhdrChunk;
use crate::png_types::{PixelFormat, Result};

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
    pub(crate) fn from_format_and_data(
        width: u32,
        height: u32,
        format: &PixelFormat,
        data: &[u8],
        interlaced: bool,
    ) -> Result<Self> {
        let interlace_method = u8::from(interlaced);
        match format {
            PixelFormat::Gray1
            | PixelFormat::Gray2
            | PixelFormat::Gray4
            | PixelFormat::Gray8
            | PixelFormat::Gray16Be => {
                let bd = format.bit_depth();
                let bpp = if bd < 8 { 1 } else { format.bytes_per_pixel() };
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, bd, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, bd, bpp, format)
                };
                Ok(Self {
                    bit_depth: bd,
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::GrayAlpha8 => {
                let bpp = 2;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, 8, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, 8, bpp, format)
                };
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE_ALPHA,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::GrayAlpha16Be => {
                let bpp = 4;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, 16, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, 16, bpp, format)
                };
                Ok(Self {
                    bit_depth: 16,
                    color_type: IhdrChunk::COLOR_TYPE_GRAYSCALE_ALPHA,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::Rgb8 => {
                let bpp = 3;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, 8, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, 8, bpp, format)
                };
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_RGB,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::Rgb16Be => {
                let bpp = 6;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, 16, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, 16, bpp, format)
                };
                Ok(Self {
                    bit_depth: 16,
                    color_type: IhdrChunk::COLOR_TYPE_RGB,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::Rgba8 => {
                let bpp = 4;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, 8, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, 8, bpp, format)
                };
                Ok(Self {
                    bit_depth: 8,
                    color_type: IhdrChunk::COLOR_TYPE_RGBA,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::Rgba16Be => {
                let bpp = 8;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, 16, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, 16, bpp, format)
                };
                Ok(Self {
                    bit_depth: 16,
                    color_type: IhdrChunk::COLOR_TYPE_RGBA,
                    interlace_method,
                    filtered_data,
                    palette: None,
                    trns: None,
                })
            }
            PixelFormat::Indexed1 { palette, trns }
            | PixelFormat::Indexed2 { palette, trns }
            | PixelFormat::Indexed4 { palette, trns }
            | PixelFormat::Indexed8 { palette, trns } => {
                let bd = format.bit_depth();
                let bpp = 1;
                let filtered_data = if interlaced {
                    build_scanline_filtered_data_adam7(width, height, data, bd, bpp, format)
                } else {
                    build_scanline_filtered_data(width, height, data, bd, bpp, format)
                };
                // Convert flat palette to [[u8; 3]]
                let (palette_chunks, _) = palette.as_chunks::<3>();
                let palette_arr: Vec<[u8; 3]> = palette_chunks.to_vec();
                let trns_vec = trns.clone();
                Ok(Self {
                    bit_depth: bd,
                    color_type: IhdrChunk::COLOR_TYPE_INDEXED,
                    interlace_method,
                    filtered_data,
                    palette: Some(palette_arr),
                    trns: trns_vec,
                })
            }
        }
    }
}

/// Build filtered scanline data for a non-interlaced image.
fn build_scanline_filtered_data(
    width: u32,
    height: u32,
    data: &[u8],
    bit_depth: u8,
    bpp: usize,
    format: &PixelFormat,
) -> Vec<u8> {
    let bytes_per_pixel = format.bytes_per_pixel();
    let needs_packing = bit_depth < 8;
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    for row in 0..height as usize {
        raw_row.clear();
        let row_start = row * width as usize * bytes_per_pixel;
        let row_end = row_start + width as usize * bytes_per_pixel;
        let row_data = &data[row_start..row_end];
        if needs_packing {
            pack_samples_to(&mut raw_row, row_data, bit_depth);
        } else {
            raw_row.extend_from_slice(row_data);
        }
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

/// Build filtered scanline data for an Adam7 interlaced image.
fn build_scanline_filtered_data_adam7(
    width: u32,
    height: u32,
    data: &[u8],
    bit_depth: u8,
    bpp: usize,
    format: &PixelFormat,
) -> Vec<u8> {
    let bytes_per_pixel = format.bytes_per_pixel();
    let needs_packing = bit_depth < 8;
    let mut filtered = Vec::new();
    let mut raw_row = Vec::new();
    let mut prev_row: Vec<u8> = Vec::new();
    let mut pass_row_data = Vec::new();
    for pass in ADAM7_PASSES {
        let pass_width = adam7_axis_size(width, pass.x_start, pass.x_step);
        let pass_height = adam7_axis_size(height, pass.y_start, pass.y_step);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        prev_row.clear();
        for pass_y in 0..pass_height as usize {
            raw_row.clear();
            pass_row_data.clear();
            let y = pass.y_start as usize + pass_y * pass.y_step as usize;
            for pass_x in 0..pass_width as usize {
                let x = pass.x_start as usize + pass_x * pass.x_step as usize;
                let src = (y * width as usize + x) * bytes_per_pixel;
                pass_row_data.extend_from_slice(&data[src..src + bytes_per_pixel]);
            }
            if needs_packing {
                pack_samples_to(&mut raw_row, &pass_row_data, bit_depth);
            } else {
                raw_row.extend_from_slice(&pass_row_data);
            }
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

    let (best_filter, best_cost) = [
        (0u8, none_cost),
        (1, sub_cost),
        (2, up_cost),
        (4, paeth_cost),
    ]
    .into_iter()
    .min_by_key(|&(_, cost)| cost)
    .expect("bug: filter candidate set must be non-empty");

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
