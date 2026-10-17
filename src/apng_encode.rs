//! Encoding of [`crate::Animation`] into APNG bytes.
//!
//! The public API exposes frames as full-canvas images (see
//! [`crate::Animation`]), so encoding never needs sub-rectangles or dispose/
//! blend operations: every frame is written as a full-canvas `SOURCE` frame
//! with `NONE` dispose. Frame 0 goes into `IDAT` (with its `fcTL` before the
//! `IDAT`), and each later frame becomes a `fcTL`/`fdAT` pair.
//!
//! Interlacing is not part of the frozen animation API, so encoded frames are
//! always non-interlaced.

use alloc::format;
use alloc::vec::Vec;

use crate::animation::{Animation, Frame, PlayCount};
use crate::chunk::{
    AcTlChunk, FcTlChunk, FdAtChunk, IdatChunk, IendChunk, IhdrChunk, PlteChunk, TrnsChunk,
};
use crate::png::PNG_SIGNATURE;
use crate::png_encode::EncodedImage;
use crate::png_types::{Error, Result};

/// Encodes `animation` into APNG bytes.
///
/// Every frame is written as a full-canvas image, so the result round-trips
/// through [`crate::decode_animation`] without any sub-rectangle bookkeeping.
pub(crate) fn encode_animation(animation: &Animation) -> Result<Vec<u8>> {
    if animation.frames.is_empty() {
        return Err(Error::InvalidData(
            "an animation must have at least one frame".into(),
        ));
    }
    if animation.width == 0 || animation.height == 0 {
        return Err(Error::InvalidData(
            "animation dimensions must be non-zero".into(),
        ));
    }

    let expected_len = animation
        .pixel_format
        .data_len(animation.width, animation.height);
    for (index, frame) in animation.frames.iter().enumerate() {
        if frame.pixels.len() != expected_len {
            return Err(Error::InvalidData(
                format!(
                    "frame {index} has {} bytes, expected {expected_len}",
                    frame.pixels.len()
                )
                .into(),
            ));
        }
    }

    let first = encode_frame(animation, &animation.frames[0].pixels)?;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&PNG_SIGNATURE);
    IhdrChunk {
        width: animation.width,
        height: animation.height,
        bit_depth: first.bit_depth,
        color_type: first.color_type,
        interlace_method: 0,
    }
    .append_to(&mut bytes);
    if let Some(palette) = first.palette.as_deref() {
        PlteChunk { palette }.append_to(&mut bytes);
    }
    if let Some(trns) = first.trns.as_deref() {
        TrnsChunk { data: trns }.append_to(&mut bytes);
    }

    AcTlChunk {
        num_frames: animation.frames.len() as u32,
        num_plays: num_plays(animation),
    }
    .append_to(&mut bytes);

    // Frame 0 is carried by IDAT, so its fcTL comes first (sequence number 0).
    let mut sequence: u32 = 0;
    frame_control_chunk(animation, &animation.frames[0], sequence).append_to(&mut bytes);
    sequence += 1;
    IdatChunk {
        filtered_data: &first.filtered_data,
    }
    .append_to(&mut bytes)?;

    for frame in animation.frames.iter().skip(1) {
        let encoded = encode_frame(animation, &frame.pixels)?;
        frame_control_chunk(animation, frame, sequence).append_to(&mut bytes);
        sequence += 1;
        // One fdAT per frame: splitting a zlib stream across chunks is a
        // decoding concern, and a single fragment is always valid.
        let zlib = crate::chunk::zlib_wrap(&encoded.filtered_data)?;
        FdAtChunk {
            sequence_number: sequence,
            data: &zlib,
        }
        .append_to(&mut bytes);
        sequence += 1;
    }

    IendChunk.append_to(&mut bytes);
    Ok(bytes)
}

/// Builds the `fcTL` control data for `frame`, using `sequence` as its sequence
/// number. The encoded stream has no sub-rectangles, so every frame covers the
/// whole canvas.
fn frame_control_chunk(animation: &Animation, frame: &Frame, sequence: u32) -> FcTlChunk {
    FcTlChunk {
        sequence_number: sequence,
        width: animation.width,
        height: animation.height,
        x_offset: 0,
        y_offset: 0,
        delay_num: frame.delay.num,
        delay_den: frame.delay.den.get(),
        dispose_op: FcTlChunk::DISPOSE_OP_NONE,
        blend_op: FcTlChunk::BLEND_OP_SOURCE,
    }
}

/// Encodes one frame's pixels into filtered scanline data plus the header
/// fields that describe them.
fn encode_frame(animation: &Animation, pixels: &[u8]) -> Result<EncodedImage> {
    EncodedImage::from_format_and_data(
        animation.width,
        animation.height,
        &animation.pixel_format,
        pixels,
        false,
    )
}

/// The `acTL.num_plays` value: `0` means loop forever.
fn num_plays(animation: &Animation) -> u32 {
    match animation.play_count {
        PlayCount::Infinite => 0,
        PlayCount::Finite(count) => count.get(),
    }
}
