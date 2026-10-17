//! Animated PNG (APNG) types and the public decode entry point.
//!
//! APNG is a backwards-compatible extension of PNG: a static viewer shows a
//! single poster image, while an APNG-aware decoder plays a sequence of frames.
//! `nopng` presents animations as plain data — a canvas size, a pixel format,
//! a play count, and a list of full-canvas [`Frame`]s. The `acTL`/`fcTL`/`fdAT`
//! bookkeeping (frame rectangles, dispose/blend operations, sequence numbers)
//! is handled internally and is deliberately not exposed.

use alloc::vec::Vec;
use core::num::{NonZeroU16, NonZeroU32};

use crate::apng::parse_animation;
use crate::apng_decode::composite;
use crate::png_types::{PixelFormat, Result};

/// The presentation duration of a frame, in seconds: `num / den`.
///
/// The delay is kept as a rational number because APNG stores it that way and
/// because decimal or floating-point conversion would be lossy — `1/3` and
/// `1/65535` have no exact representation in seconds. Divide the fields
/// yourself if you need a single number.
///
/// `den` is a [`NonZeroU16`], so the "denominator of zero" that APNG allows on
/// the wire cannot be represented here. Decoding normalizes an on-the-wire
/// `delay_den` of `0` to `100`, which is the interpretation the specification
/// prescribes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delay {
    /// Numerator of the delay in seconds.
    pub num: u16,
    /// Denominator of the delay in seconds.
    pub den: NonZeroU16,
}

/// A single animation frame, composited onto the full canvas.
///
/// `pixels` is a complete canvas-sized image in the [`Animation::pixel_format`]
/// of its animation, laid out like any other `nopng` pixel buffer. Frames do not
/// carry their own size or offset: an APNG frame may be a sub-rectangle drawn
/// over the previous frame, but by the time you see it here it has been expanded
/// and combined, so every frame is directly displayable on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Full-canvas pixel data in the animation's `pixel_format`.
    pub pixels: Vec<u8>,
    /// How long this frame is displayed.
    pub delay: Delay,
}

/// How many times an animation should be played.
///
/// APNG stores this as a `u32` where `0` means "loop forever". The sentinel is
/// not representable here: use [`PlayCount::Infinite`] for looping and
/// [`PlayCount::Finite`] for a bounded count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayCount {
    /// Loop forever.
    Infinite,
    /// Play exactly this many times. Always at least `1`.
    Finite(NonZeroU32),
}

impl PlayCount {
    /// Play the animation exactly once.
    ///
    /// Equivalent to `PlayCount::Finite(NonZeroU32::MIN)`.
    pub const ONCE: PlayCount = PlayCount::Finite(NonZeroU32::MIN);
}

/// A decoded (or to-be-encoded) animated PNG.
///
/// Unlike a static image, an animation carries a width and height, a play
/// count, and an ordered list of composited frames.
///
/// The width and height are stored once here rather than per frame, because
/// every frame is that size. On the wire an APNG frame may be a sub-rectangle
/// drawn over the previous frame; decoding expands each one to the full canvas,
/// so no frame in this type needs a size or an offset of its own.
///
/// All frames are expected to share one `pixel_format`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation {
    /// Width in pixels. Every frame is this wide. Never zero.
    pub width: NonZeroU32,
    /// Height in pixels. Every frame is this tall. Never zero.
    pub height: NonZeroU32,
    /// Pixel format shared by every frame's buffer.
    pub pixel_format: PixelFormat,
    /// How many times the animation should play.
    pub play_count: PlayCount,
    /// The frames, in presentation order.
    pub frames: Vec<Frame>,
}

/// Encodes an [`Animation`] into APNG bytes.
///
/// Every frame is written as a full-canvas image with `SOURCE` blend and `NONE`
/// dispose, so [`decode_animation`] reconstructs the frames exactly. The
/// resulting bytes are always non-interlaced.
///
/// # Errors
///
/// Returns [`Error::InvalidData`](crate::Error::InvalidData) if `animation` has
/// no frames, or if a frame's pixel buffer is not the length
/// [`PixelFormat::data_len`] prescribes for the canvas.
pub fn encode_animation(animation: &Animation) -> Result<Vec<u8>> {
    crate::apng_encode::encode_animation(animation)
}

/// Decodes APNG bytes into an [`Animation`].
///
/// Every frame is returned composited onto the full canvas, so a frame can be
/// displayed without consulting the others. The result is therefore directly
/// usable: every [`Frame::pixels`] holds one complete `width` by `height` image,
/// so no frame needs to be combined with its neighbours before display. Read the
/// frames in the order they appear, starting from a transparent black canvas.
///
/// The frames are always in [`PixelFormat::Rgba16Be`] — the crate's lossless
/// common denominator. Compositing mixes colors from frames that may use
/// different formats, so the result is materialized in a single format rather
/// than in any one source's. Convert it with
/// [`reformat_pixels`](crate::reformat_pixels) if you want something narrower
/// such as `Rgba8`.
///
/// The decoded frames reflect the source's dispose and blend operations; the
/// operations themselves are not exposed because they have already been applied.
///
/// # Examples
///
/// Encode an animation and decode it again:
///
/// ```
/// let animation = nopng::Animation {
///     width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
///     height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
///     pixel_format: nopng::PixelFormat::Rgba8,
///     play_count: nopng::PlayCount::Infinite,
///     frames: vec![
///         nopng::Frame {
///             pixels: vec![255, 0, 0, 255].repeat(4),
///             delay: nopng::Delay {
///                 num: 1,
///                 den: std::num::NonZeroU16::new(10).expect("10 is non-zero"),
///             },
///         },
///     ],
/// };
/// let bytes = nopng::encode_animation(&animation)?;
/// let decoded = nopng::decode_animation(&bytes)?;
/// assert_eq!(decoded.frames.len(), 1);
/// # Ok::<(), nopng::Error>(())
/// ```
///
/// # Errors
///
/// Returns [`Error::InvalidData`](crate::Error::InvalidData) if `bytes` is not a
/// valid APNG — for example if it has no `acTL` chunk (a static PNG), if chunk
/// ordering or sequence numbers are broken, if a frame is not fully inside the
/// canvas, or if the first frame does not cover the canvas.
pub fn decode_animation(bytes: &[u8]) -> Result<Animation> {
    let raw = parse_animation(bytes)?;
    let composited = composite(raw)?;

    let mut frames = Vec::with_capacity(composited.frames.len());
    for frame in composited.frames {
        frames.push(Frame {
            pixels: frame.pixels,
            delay: Delay {
                num: frame.delay_num,
                den: normalize_delay_den(frame.delay_den),
            },
        });
    }

    Ok(Animation {
        width: non_zero_dimension(composited.width),
        height: non_zero_dimension(composited.height),
        pixel_format: PixelFormat::Rgba16Be,
        play_count: play_count_from_u32(composited.num_plays),
        frames,
    })
}

/// Narrows a decoded APNG dimension to [`NonZeroU32`].
///
/// The APNG walker and compositor work in plain `u32`, but `IHDR` and `fcTL`
/// dimensions are already required to be non-zero there, so this conversion
/// cannot fail on a decoded animation.
fn non_zero_dimension(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).expect("bug: apng_decode accepted a zero dimension")
}

/// APNG treats a `delay_den` of `0` as `100`; the type forbids zero, so the
/// sentinel is resolved here.
fn normalize_delay_den(den: u16) -> NonZeroU16 {
    NonZeroU16::new(den).unwrap_or(NonZeroU16::new(100).expect("100 is non-zero"))
}

fn play_count_from_u32(num_plays: u32) -> PlayCount {
    match NonZeroU32::new(num_plays) {
        Some(finite) => PlayCount::Finite(finite),
        None => PlayCount::Infinite,
    }
}
