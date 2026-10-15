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
use crate::pixel_reformat::reformat;
use crate::png_types::{Error, PixelFormat, Result};

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
    /// Denominator of the delay in seconds. Always non-zero.
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
    /// Equivalent to `PlayCount::Finite(NonZeroU32::new(1).expect("1 is non-zero"))`,
    /// but available in `const` contexts.
    pub const ONCE: PlayCount = match NonZeroU32::new(1) {
        Some(one) => PlayCount::Finite(one),
        // `NonZeroU32::new(1)` is never `None`; this arm exists only because
        // `Option::unwrap` is not `const`. It is unreachable.
        None => PlayCount::Infinite,
    };
}

/// A decoded (or to-be-encoded) animated PNG.
///
/// Unlike a static image, an animation carries a **canvas** size that is
/// independent of any individual frame's rectangle, plus a play count and the
/// ordered list of composited frames.
///
/// All frames are expected to share one `pixel_format`, and `width`/`height`
/// describe every frame's buffer (each frame is full-canvas).
///
/// # Examples
///
/// ```
/// # let bytes = {
/// #     let spec = nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8);
/// #     nopng::encode_image(&spec, &[255, 0, 0, 255])?
/// # };
/// // A static PNG fails to decode as an animation: APNG requires an acTL chunk.
/// assert!(nopng::decode_animation(&bytes, nopng::PixelFormat::Rgba8).is_err());
/// # Ok::<(), nopng::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation {
    /// Canvas width in pixels. Every frame is this wide.
    pub width: u32,
    /// Canvas height in pixels. Every frame is this tall.
    pub height: u32,
    /// Pixel format shared by every frame's buffer.
    pub pixel_format: PixelFormat,
    /// How many times the animation should play.
    pub play_count: PlayCount,
    /// The frames, in presentation order.
    pub frames: Vec<Frame>,
}

impl Animation {
    /// Expected byte length of one frame's pixel buffer.
    ///
    /// # Panics
    ///
    /// Panics if the total size overflows `usize`.
    pub fn frame_data_len(&self) -> usize {
        self.pixel_format.data_len(self.width, self.height)
    }
}

/// Decodes an APNG byte stream into an [`Animation`].
///
/// Every frame is returned composited onto the full canvas and converted to
/// `format`, so a frame can be displayed without consulting the others. Frames
/// are in presentation order, and the canvas starts as transparent black.
///
/// The decoded frames reflect the source's dispose and blend operations; the
/// operations themselves are not exposed because they have already been applied.
///
/// # Errors
///
/// Returns [`Error::InvalidData`] if `bytes` is not a valid APNG — for example
/// if it has no `acTL` chunk (a static PNG), if chunk ordering or sequence
/// numbers are broken, if a frame is not fully inside the canvas, or if the
/// first frame does not cover the canvas.
///
/// Returns [`Error::Unsupported`] if `format` is an indexed format, since
/// compositing works in a full-color space and cannot target a palette.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), nopng::Error> {
/// # let png = nopng::encode_image(
/// #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
/// #     &[255, 0, 0, 255],
/// # )?;
/// // A plain PNG has no `acTL` chunk, so it is not an animation.
/// assert!(nopng::decode_animation(&png, nopng::PixelFormat::Rgba8).is_err());
/// # Ok(())
/// # }
/// ```
///
/// ```
/// # fn main() -> Result<(), nopng::Error> {
/// # let png = nopng::encode_image(
/// #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
/// #     &[255, 0, 0, 255],
/// # )?;
/// // Indexed output needs a palette, which compositing does not produce.
/// let error = nopng::decode_animation(&png, nopng::PixelFormat::Indexed8 {
///     palette: vec![0, 0, 0],
///     trns: None,
/// });
/// assert!(matches!(error, Err(nopng::Error::Unsupported(_))));
/// # Ok(())
/// # }
/// ```
pub fn decode_animation(bytes: &[u8], format: PixelFormat) -> Result<Animation> {
    if is_indexed(&format) {
        return Err(Error::Unsupported(
            "compositing cannot target an indexed pixel format".into(),
        ));
    }

    let raw = parse_animation(bytes)?;
    let composited = composite(raw)?;

    let mut frames = Vec::with_capacity(composited.frames.len());
    for frame in composited.frames {
        let pixels = reformat(&PixelFormat::Rgba16Be, &frame.pixels, &format)?;
        frames.push(Frame {
            pixels,
            delay: Delay {
                num: frame.delay_num,
                den: normalize_delay_den(frame.delay_den),
            },
        });
    }

    Ok(Animation {
        width: composited.width,
        height: composited.height,
        pixel_format: format,
        play_count: play_count_from_u32(composited.num_plays),
        frames,
    })
}

fn is_indexed(format: &PixelFormat) -> bool {
    matches!(
        format,
        PixelFormat::Indexed1 { .. }
            | PixelFormat::Indexed2 { .. }
            | PixelFormat::Indexed4 { .. }
            | PixelFormat::Indexed8 { .. }
    )
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
