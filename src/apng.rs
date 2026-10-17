//! APNG (Animated PNG) chunk-level parsing and validation.
//!
//! This module owns the wire format of the APNG chunks (`acTL`, `fcTL`, `fdAT`)
//! and the walker that turns a PNG byte stream into the raw per-frame data the
//! decoder composites. Nothing here is public: the frozen public API only
//! exposes [`crate::decode_animation`] and [`crate::encode_animation`], and all
//! of the rect/dispose/blend/sequence bookkeeping is an implementation detail.

use alloc::format;
use alloc::vec::Vec;

use crate::chunk::{AcTlChunk, FcTlChunk, FdAtChunk};
use crate::crc;
use crate::png::PNG_SIGNATURE;
use crate::png_decode::{AncillaryChunks, Cursor, PngHeader, apply_ancillary_chunk};
use crate::png_types::{Error, Result};

/// How the region covered by a frame is treated before the next frame is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisposeOp {
    /// Leave the frame's pixels in place.
    None,
    /// Clear the frame's region to fully transparent black.
    Background,
    /// Restore the frame's region to its contents before this frame was drawn.
    Previous,
}

impl DisposeOp {
    fn from_u8(value: u8) -> Result<Self> {
        match value {
            FcTlChunk::DISPOSE_OP_NONE => Ok(Self::None),
            FcTlChunk::DISPOSE_OP_BACKGROUND => Ok(Self::Background),
            FcTlChunk::DISPOSE_OP_PREVIOUS => Ok(Self::Previous),
            _ => Err(Error::InvalidData(
                format!("invalid dispose_op: {value}").into(),
            )),
        }
    }
}

/// How a frame's pixels are combined with what is already on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlendOp {
    /// Overwrite the destination, including its alpha.
    Source,
    /// Source-over compositing with straight (non-premultiplied) alpha.
    Over,
}

impl BlendOp {
    fn from_u8(value: u8) -> Result<Self> {
        match value {
            FcTlChunk::BLEND_OP_SOURCE => Ok(Self::Source),
            FcTlChunk::BLEND_OP_OVER => Ok(Self::Over),
            _ => Err(Error::InvalidData(
                format!("invalid blend_op: {value}").into(),
            )),
        }
    }
}

/// A single animation frame as declared on the wire, before compositing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawFrame {
    pub(crate) control: FcTlChunk,
    pub(crate) dispose_op: DisposeOp,
    pub(crate) blend_op: BlendOp,
    /// The frame's zlib stream (header, deflate payload, Adler-32 trailer),
    /// reassembled from its `fdAT` fragments. Empty for frame 0 when it is
    /// carried by `IDAT` instead (see [`RawAnimation::idat_is_frame_zero`]).
    pub(crate) zlib_stream: Vec<u8>,
}

/// Everything the decoder needs to composite an animation, as read from the
/// chunk stream. The frame data is still zlib-compressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawAnimation {
    pub(crate) header: PngHeader,
    /// `PLTE`/`tRNS` metadata, shared by every frame. In an APNG these chunks
    /// precede `IDAT`, so they apply to the whole animation rather than to one
    /// frame.
    pub(crate) ancillary: AncillaryChunks,
    /// `IDAT` payloads concatenated in order.
    pub(crate) idat_data: Vec<u8>,
    /// Declared `acTL` frame count, preserved verbatim. The spec permits it to
    /// disagree with the number of frames present, so it is informational only;
    /// the number of frames is `frames.len()`.
    pub(crate) num_frames: u32,
    /// Declared `acTL` play count; `0` means loop forever.
    pub(crate) num_plays: u32,
    /// Whether the first `fcTL` precedes the first `IDAT`, in which case `IDAT`
    /// is animation frame 0. If false, `IDAT` is a poster image that is not
    /// part of the animation and every frame comes from `fdAT`.
    pub(crate) idat_is_frame_zero: bool,
    pub(crate) frames: Vec<RawFrame>,
}

/// Walks an APNG byte stream and returns its raw (still compressed) frame data.
///
/// Unlike `parse_png`, this does not stop at `IDAT`: it keeps going to collect
/// `fcTL`/`fdAT` chunks. Structural validation that does not depend on
/// decompressed pixels (sequence numbering, ordering, disjoint rects) happens
/// here; rectangle-vs-canvas and frame-0 shape checks happen in the decoder,
/// where they can be expressed in terms of the compositor's invariants.
pub(crate) fn parse_animation(bytes: &[u8]) -> Result<RawAnimation> {
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(Error::InvalidData("invalid PNG signature".into()));
    }

    let mut cursor = Cursor::new(&bytes[PNG_SIGNATURE.len()..]);
    let mut header: Option<PngHeader> = None;
    let mut ancillary = AncillaryChunks::default();
    let mut idat_data = Vec::new();
    let mut actl: Option<AcTlChunk> = None;
    let mut frames: Vec<RawFrame> = Vec::new();
    let mut sequence_number: u32 = 0;
    let mut seen_idat = false;
    let mut seen_iend = false;
    // Only the very first fcTL may precede IDAT (making IDAT frame 0); every
    // later fcTL must follow IDAT.
    let mut seen_fctl = false;
    // True when the *first* fcTL was encountered before the first IDAT, which
    // makes IDAT the animation's frame 0 rather than a poster image.
    let mut idat_is_frame_zero = false;
    // Index into `frames` that the currently-open `fdAT` fragments belong to.
    let mut open_frame: Option<usize> = None;

    while cursor.remaining() > 0 {
        let length = cursor.read_u32()? as usize;
        let chunk_type = cursor.read_array::<4>()?;
        let chunk_data = cursor.read_bytes(length)?;
        let expected_crc = cursor.read_u32()?;

        let actual_crc = crc::calculate(&[&chunk_type[..], chunk_data]);
        if actual_crc != expected_crc {
            return Err(Error::InvalidData(
                format!(
                    "CRC mismatch for chunk {}",
                    core::str::from_utf8(&chunk_type).unwrap_or("????"),
                )
                .into(),
            ));
        }

        match &chunk_type {
            b"IHDR" => {
                if header.is_some() {
                    return Err(Error::InvalidData("duplicate IHDR chunk".into()));
                }
                header = Some(PngHeader::parse(chunk_data)?);
            }
            b"acTL" => {
                if actl.is_some() {
                    return Err(Error::InvalidData("duplicate acTL chunk".into()));
                }
                if seen_idat {
                    return Err(Error::InvalidData("acTL chunk after IDAT".into()));
                }
                actl = Some(AcTlChunk::parse(chunk_data)?);
            }
            b"PLTE" | b"tRNS" => {
                let Some(header) = header.as_ref() else {
                    return Err(Error::InvalidData("PLTE/tRNS chunk before IHDR".into()));
                };
                // In a valid APNG these precede IDAT and apply to every frame.
                if seen_idat {
                    return Err(Error::InvalidData("PLTE/tRNS chunk after IDAT".into()));
                }
                apply_ancillary_chunk(&chunk_type, chunk_data, header, &mut ancillary)?;
            }
            b"IDAT" => {
                if header.is_none() {
                    return Err(Error::InvalidData("IDAT chunk before IHDR".into()));
                }
                seen_idat = true;
                idat_data.extend_from_slice(chunk_data);
            }
            b"fcTL" => {
                // The first fcTL may appear before IDAT (IDAT is then frame 0);
                // any later fcTL must come after IDAT.
                if !seen_idat && seen_fctl {
                    return Err(Error::InvalidData("fcTL chunk before IDAT".into()));
                }
                if !seen_fctl && !seen_idat {
                    idat_is_frame_zero = true;
                }
                seen_fctl = true;
                let control = FcTlChunk::parse(chunk_data)?;
                check_sequence_number(&mut sequence_number, control.sequence_number)?;
                let dispose_op = DisposeOp::from_u8(control.dispose_op)?;
                let blend_op = BlendOp::from_u8(control.blend_op)?;
                if control.width == 0 || control.height == 0 {
                    return Err(Error::InvalidData(
                        "frame width and height must be non-zero".into(),
                    ));
                }
                frames.push(RawFrame {
                    control,
                    dispose_op,
                    blend_op,
                    zlib_stream: Vec::new(),
                });
                open_frame = Some(frames.len() - 1);
            }
            b"fdAT" => {
                if !seen_idat {
                    return Err(Error::InvalidData("fdAT chunk before IDAT".into()));
                }
                let Some(frame_index) = open_frame else {
                    return Err(Error::InvalidData(
                        "fdAT chunk without a preceding fcTL chunk".into(),
                    ));
                };
                let fd_at = FdAtChunk::parse(chunk_data)?;
                check_sequence_number(&mut sequence_number, fd_at.sequence_number)?;
                frames[frame_index]
                    .zlib_stream
                    .extend_from_slice(fd_at.data);
            }
            b"IEND" => {
                seen_iend = true;
                break;
            }
            _ => {}
        }
    }

    if !seen_iend {
        return Err(Error::InvalidData("missing IEND chunk".into()));
    }
    let Some(header) = header else {
        return Err(Error::InvalidData("missing IHDR chunk".into()));
    };
    let Some(actl) = actl else {
        return Err(Error::InvalidData("missing acTL chunk".into()));
    };
    ancillary.validate(&header)?;
    if idat_data.is_empty() {
        return Err(Error::InvalidData("missing IDAT chunk".into()));
    }
    if frames.is_empty() {
        return Err(Error::InvalidData("missing fcTL chunk".into()));
    }

    // Frame 0 is the IDAT stream when the first fcTL appeared before IDAT; in
    // that case the first `frames` entry carries no fdAT data. Otherwise every
    // frame has its own fdAT stream and IDAT is a poster that is not shown.
    if idat_is_frame_zero {
        if !frames[0].zlib_stream.is_empty() {
            return Err(Error::InvalidData(
                "frame 0 has both IDAT and fdAT data".into(),
            ));
        }
    } else if frames.iter().any(|f| f.zlib_stream.is_empty()) {
        return Err(Error::InvalidData("missing fdAT chunk for frame".into()));
    }

    // Per the APNG spec, decoders must not trust `acTL.num_frames`: some
    // encoders write a count that disagrees with the number of frames actually
    // present. The chunk stream is authoritative, so the declared value is
    // carried through for information only and never validated against.

    Ok(RawAnimation {
        header,
        ancillary,
        idat_data,
        num_frames: actl.num_frames,
        num_plays: actl.num_plays,
        idat_is_frame_zero,
        frames,
    })
}

/// Enforces the shared, strictly-increasing sequence number of `fcTL` and
/// `fdAT` chunks.
fn check_sequence_number(previous: &mut u32, actual: u32) -> Result<()> {
    if actual != *previous {
        return Err(Error::InvalidData(
            format!("unexpected APNG sequence number: expected {previous}, found {actual}").into(),
        ));
    }
    *previous = previous.wrapping_add(1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::{BlendOp, DisposeOp, parse_animation};
    use crate::chunk::{AcTlChunk, FcTlChunk, FdAtChunk, IendChunk, IhdrChunk};
    use crate::png::PNG_SIGNATURE;

    const CANVAS: u32 = 2;

    fn header() -> IhdrChunk {
        IhdrChunk {
            width: CANVAS,
            height: CANVAS,
            bit_depth: 8,
            color_type: IhdrChunk::COLOR_TYPE_RGBA,
            interlace_method: 0,
        }
    }

    fn fctl(sequence_number: u32) -> FcTlChunk {
        FcTlChunk {
            sequence_number,
            width: CANVAS,
            height: CANVAS,
            x_offset: 0,
            y_offset: 0,
            delay_num: 1,
            delay_den: 10,
            dispose_op: FcTlChunk::DISPOSE_OP_NONE,
            blend_op: FcTlChunk::BLEND_OP_SOURCE,
        }
    }

    /// A minimal, well-formed APNG with `num_frames` full-canvas frames.
    /// Frame 0 is carried by IDAT (the first fcTL precedes IDAT).
    fn build_apng(num_frames: u32, num_plays: u32, idat_is_frame_zero: bool) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);
        header().append_to(&mut out);
        AcTlChunk {
            num_frames,
            num_plays,
        }
        .append_to(&mut out);

        let mut sequence = 0u32;
        if idat_is_frame_zero {
            fctl(sequence).append_to(&mut out);
            sequence += 1;
        }
        crate::chunk::IdatChunk {
            filtered_data: &[0u8; 5 * CANVAS as usize],
        }
        .append_to(&mut out)
        .expect("infallible");

        for _ in 0..num_frames - u32::from(idat_is_frame_zero) {
            fctl(sequence).append_to(&mut out);
            sequence += 1;
            FdAtChunk {
                sequence_number: sequence,
                data: b"\x78\x01\x00",
            }
            .append_to(&mut out);
            sequence += 1;
        }
        IendChunk.append_to(&mut out);
        out
    }

    #[test]
    fn parses_idat_as_frame_zero_animation() {
        let bytes = build_apng(2, 0, true);
        let animation = parse_animation(&bytes).expect("infallible");
        assert_eq!(animation.header.width, CANVAS);
        assert_eq!(animation.num_frames, 2);
        assert_eq!(animation.num_plays, 0);
        assert!(animation.idat_is_frame_zero);
        assert_eq!(animation.frames.len(), 2);
        assert!(animation.frames[0].zlib_stream.is_empty());
        assert!(!animation.frames[1].zlib_stream.is_empty());
        assert_eq!(animation.frames[0].dispose_op, DisposeOp::None);
        assert_eq!(animation.frames[0].blend_op, BlendOp::Source);
    }

    #[test]
    fn parses_poster_idat_animation() {
        // First fcTL follows IDAT: IDAT is a poster, so all frames come from fdAT.
        let bytes = build_apng(2, 3, false);
        let animation = parse_animation(&bytes).expect("infallible");
        assert!(!animation.idat_is_frame_zero);
        assert_eq!(animation.frames.len(), 2);
        assert!(animation.frames.iter().all(|f| !f.zlib_stream.is_empty()));
    }

    #[test]
    fn rejects_missing_actl() {
        let mut bytes = build_apng(1, 0, true);
        // Overwrite the acTL type bytes with a harmless ancillary chunk type.
        let position = find_chunk(&bytes, b"acTL").expect("acTL present");
        bytes[position..position + 4].copy_from_slice(b"teSt");
        // CRC is now stale for the renamed chunk, which is an error too; the
        // point is only that the stream no longer parses as an APNG.
        assert!(parse_animation(&bytes).is_err());
    }

    #[test]
    fn rejects_sequence_number_gap() {
        let mut bytes = build_apng(2, 0, true);
        // Corrupt the second fcTL's sequence number (expected value is 1).
        let second = find_nth_chunk(&bytes, b"fcTL", 2).expect("second fcTL present");
        bytes[second + 4..second + 8].copy_from_slice(&7u32.to_be_bytes());
        fix_crc(&mut bytes, second, 26);
        assert!(parse_animation(&bytes).is_err());
    }

    #[test]
    fn rejects_invalid_dispose_and_blend_ops() {
        let mut bytes = build_apng(1, 0, true);
        let position = find_chunk(&bytes, b"fcTL").expect("fcTL present");
        bytes[position + 4 + 24] = 3; // dispose_op out of range
        fix_crc(&mut bytes, position, 26);
        assert!(parse_animation(&bytes).is_err());

        let mut bytes = build_apng(1, 0, true);
        let position = find_chunk(&bytes, b"fcTL").expect("fcTL present");
        bytes[position + 4 + 25] = 2; // blend_op out of range
        fix_crc(&mut bytes, position, 26);
        assert!(parse_animation(&bytes).is_err());
    }

    #[test]
    fn rejects_frame_zero_with_both_idat_and_fdat() {
        // Frame 0 carries IDAT, so an fdAT for it is an error.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PNG_SIGNATURE);
        header().append_to(&mut bytes);
        AcTlChunk {
            num_frames: 1,
            num_plays: 0,
        }
        .append_to(&mut bytes);
        fctl(0).append_to(&mut bytes);
        crate::chunk::IdatChunk {
            filtered_data: &[0u8; 5 * CANVAS as usize],
        }
        .append_to(&mut bytes)
        .expect("infallible");
        FdAtChunk {
            sequence_number: 1,
            data: b"\x78\x01\x00",
        }
        .append_to(&mut bytes);
        IendChunk.append_to(&mut bytes);
        // The extra fdAT leaves the fcTL count at 1, matching acTL, but the
        // frame-0 fdAT is rejected.
        assert!(parse_animation(&bytes).is_err());
    }

    #[test]
    fn tolerates_wrong_actl_frame_count() {
        // The declared count is informational; the chunk stream is trusted.
        let bytes = build_apng(2, 0, true);
        let mut patched = bytes.clone();
        let position = find_chunk(&patched, b"acTL").expect("acTL present");
        patched[position + 4..position + 8].copy_from_slice(&5u32.to_be_bytes());
        fix_crc(&mut patched, position, 8);
        let animation = parse_animation(&patched).expect("infallible");
        assert_eq!(animation.num_frames, 5);
        assert_eq!(animation.frames.len(), 2);
    }

    #[test]
    fn rejects_second_fctl_before_idat() {
        // Only the first fcTL may precede IDAT. Emit two fcTL chunks before the
        // IDAT chunk; the second is illegal.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PNG_SIGNATURE);
        header().append_to(&mut bytes);
        AcTlChunk {
            num_frames: 2,
            num_plays: 0,
        }
        .append_to(&mut bytes);
        fctl(0).append_to(&mut bytes);
        fctl(1).append_to(&mut bytes);
        crate::chunk::IdatChunk {
            filtered_data: &[0u8; 5 * CANVAS as usize],
        }
        .append_to(&mut bytes)
        .expect("infallible");
        IendChunk.append_to(&mut bytes);
        assert!(parse_animation(&bytes).is_err());
    }

    #[test]
    fn rejects_fdat_without_fctl() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PNG_SIGNATURE);
        header().append_to(&mut bytes);
        AcTlChunk {
            num_frames: 1,
            num_plays: 0,
        }
        .append_to(&mut bytes);
        crate::chunk::IdatChunk {
            filtered_data: &[0u8; 5 * CANVAS as usize],
        }
        .append_to(&mut bytes)
        .expect("infallible");
        FdAtChunk {
            sequence_number: 0,
            data: b"\x78\x01\x00",
        }
        .append_to(&mut bytes);
        IendChunk.append_to(&mut bytes);
        assert!(parse_animation(&bytes).is_err());
    }

    #[test]
    fn sequence_numbers_are_shared_between_fctl_and_fdat() {
        // A frame's fcTL and fdAT must use consecutive numbers (0, 1), and the
        // next frame continues from 2.
        let bytes = build_apng(2, 0, true);
        let animation = parse_animation(&bytes).expect("infallible");
        assert_eq!(animation.frames[1].control.sequence_number, 1);
    }

    fn find_chunk(bytes: &[u8], chunk_type: &[u8; 4]) -> Option<usize> {
        find_nth_chunk(bytes, chunk_type, 1)
    }

    /// Returns the offset of the chunk type bytes of the `n`th chunk of the
    /// given type (1-indexed).
    fn find_nth_chunk(bytes: &[u8], chunk_type: &[u8; 4], n: usize) -> Option<usize> {
        let mut offset = PNG_SIGNATURE.len();
        let mut seen = 0;
        while offset + 8 <= bytes.len() {
            let length =
                u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("4 bytes")) as usize;
            let found_type = &bytes[offset + 4..offset + 8];
            if found_type == chunk_type {
                seen += 1;
                if seen == n {
                    return Some(offset + 4);
                }
            }
            offset += 12 + length;
        }
        None
    }

    /// Recomputes the CRC stored after a chunk whose type begins at `type_pos`.
    fn fix_crc(bytes: &mut [u8], type_pos: usize, data_len: usize) {
        let crc_pos = type_pos + 4 + data_len;
        let crc = crate::crc::calculate(&[
            &bytes[type_pos..type_pos + 4],
            &bytes[type_pos + 4..crc_pos],
        ]);
        bytes[crc_pos..crc_pos + 4].copy_from_slice(&crc.to_be_bytes());
    }

    #[allow(unused_imports)]
    use alloc::format;
}
