use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::error::Error as CoreError;

/// Errors returned by decoding and encoding operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The PNG uses a feature that this crate does not support.
    Unsupported(Cow<'static, str>),
    /// The PNG data is malformed or violates the specification.
    InvalidData(Cow<'static, str>),
}

/// A convenience alias for `core::result::Result<T, Error>`.
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

/// Describes the pixel layout of image data in a flat `&[u8]` buffer.
///
/// Each variant fully specifies the color model, bit depth, and byte layout of
/// the pixel data. The buffer is always row-major (top-to-bottom,
/// left-to-right).
///
/// # Byte layout
///
/// | Family | Bytes per pixel | Sample layout |
/// |--------|-----------------|---------------|
/// | `Gray1` – `Gray8` | 1 | One unpacked byte per sample (value range `0..2^depth`) |
/// | `Gray16Be` | 2 | `[hi, lo]` big-endian |
/// | `GrayAlpha8` | 2 | `[gray, alpha]` |
/// | `GrayAlpha16Be` | 4 | `[g_hi, g_lo, a_hi, a_lo]` |
/// | `Rgb8` | 3 | `[r, g, b]` |
/// | `Rgb16Be` | 6 | `[r_hi, r_lo, g_hi, g_lo, b_hi, b_lo]` |
/// | `Rgba8` | 4 | `[r, g, b, a]` |
/// | `Rgba16Be` | 8 | `[r_hi, r_lo, g_hi, g_lo, b_hi, b_lo, a_hi, a_lo]` |
/// | `Indexed1` – `Indexed8` | 1 | One unpacked palette index per byte |
///
/// Low-bit formats (`Gray1`–`Gray4`, `Indexed1`–`Indexed4`) store each
/// sample/index in a full byte for easy random access — they are **not**
/// bit-packed.
///
/// 16-bit formats use big-endian byte order, matching the PNG wire format.
///
/// # Indexed variants
///
/// `Indexed*` variants carry an embedded `palette` (flat RGB triplets) and an
/// optional `trns` (per-index alpha values). When decoding, these are populated
/// from the PNG `PLTE` and `tRNS` chunks. When encoding, the caller supplies
/// them.
///
/// # Format queries
///
/// `PixelFormat` has no public methods. Query the format by pattern matching:
///
/// ```
/// # let png_bytes = nopng::encode_image(
/// #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
/// #     &[255, 0, 0, 255],
/// # )?;
/// let (spec, pixels) = nopng::decode_image(&png_bytes, None)?;
///
/// match spec.pixel_format {
///     nopng::PixelFormat::Rgba8 => { /* 4 bytes per pixel */ }
///     nopng::PixelFormat::Gray8 => { /* 1 byte per pixel */ }
///     _ => { /* ... */ }
/// }
///
/// let is_16bit = matches!(
///     spec.pixel_format,
///     nopng::PixelFormat::Gray16Be
///         | nopng::PixelFormat::GrayAlpha16Be
///         | nopng::PixelFormat::Rgb16Be
///         | nopng::PixelFormat::Rgba16Be,
/// );
///
/// let is_indexed = matches!(
///     spec.pixel_format,
///     nopng::PixelFormat::Indexed1 { .. }
///         | nopng::PixelFormat::Indexed2 { .. }
///         | nopng::PixelFormat::Indexed4 { .. }
///         | nopng::PixelFormat::Indexed8 { .. },
/// );
/// # Ok::<(), nopng::Error>(())
/// ```
///
/// Use [`crate::ImageSpec::data_len()`] to compute the expected buffer size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    /// 1-bit grayscale (1 byte per sample, unpacked). Valid sample values: 0–1.
    Gray1,
    /// 2-bit grayscale (1 byte per sample, unpacked). Valid sample values: 0–3.
    Gray2,
    /// 4-bit grayscale (1 byte per sample, unpacked). Valid sample values: 0–15.
    Gray4,
    /// 8-bit grayscale (1 byte per sample).
    Gray8,
    /// 16-bit grayscale, big-endian (2 bytes per sample: `[hi, lo]`).
    Gray16Be,
    /// 8-bit grayscale + alpha (2 bytes per pixel: `[gray, alpha]`).
    GrayAlpha8,
    /// 16-bit grayscale + alpha, big-endian (4 bytes per pixel:
    /// `[g_hi, g_lo, a_hi, a_lo]`).
    GrayAlpha16Be,
    /// 8-bit RGB (3 bytes per pixel: `[r, g, b]`).
    Rgb8,
    /// 16-bit RGB, big-endian (6 bytes per pixel:
    /// `[r_hi, r_lo, g_hi, g_lo, b_hi, b_lo]`).
    Rgb16Be,
    /// 8-bit RGBA (4 bytes per pixel: `[r, g, b, a]`).
    Rgba8,
    /// 16-bit RGBA, big-endian (8 bytes per pixel:
    /// `[r_hi, r_lo, g_hi, g_lo, b_hi, b_lo, a_hi, a_lo]`).
    Rgba16Be,
    /// 1-bit indexed color (1 byte per index, unpacked). Valid index values: 0–1.
    Indexed1 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`). Length must be a
        /// multiple of 3, with at most 2 entries (6 bytes).
        palette: Vec<u8>,
        /// Per-index alpha values. May be shorter than the palette; missing
        /// entries are treated as fully opaque (255).
        trns: Option<Vec<u8>>,
    },
    /// 2-bit indexed color (1 byte per index, unpacked). Valid index values: 0–3.
    Indexed2 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`). Length must be a
        /// multiple of 3, with at most 4 entries (12 bytes).
        palette: Vec<u8>,
        /// Per-index alpha values. May be shorter than the palette; missing
        /// entries are treated as fully opaque (255).
        trns: Option<Vec<u8>>,
    },
    /// 4-bit indexed color (1 byte per index, unpacked). Valid index values: 0–15.
    Indexed4 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`). Length must be a
        /// multiple of 3, with at most 16 entries (48 bytes).
        palette: Vec<u8>,
        /// Per-index alpha values. May be shorter than the palette; missing
        /// entries are treated as fully opaque (255).
        trns: Option<Vec<u8>>,
    },
    /// 8-bit indexed color (1 byte per index). Valid index values: 0–255.
    Indexed8 {
        /// Flat RGB triplets (`[r, g, b, r, g, b, ...]`). Length must be a
        /// multiple of 3, with at most 256 entries (768 bytes).
        palette: Vec<u8>,
        /// Per-index alpha values. May be shorter than the palette; missing
        /// entries are treated as fully opaque (255).
        trns: Option<Vec<u8>>,
    },
}

impl PixelFormat {
    /// Bit depth of each sample as a raw `u8`.
    pub(crate) fn bit_depth(&self) -> u8 {
        match self {
            Self::Gray1 | Self::Indexed1 { .. } => 1,
            Self::Gray2 | Self::Indexed2 { .. } => 2,
            Self::Gray4 | Self::Indexed4 { .. } => 4,
            Self::Gray8 | Self::GrayAlpha8 | Self::Rgb8 | Self::Rgba8 | Self::Indexed8 { .. } => 8,
            Self::Gray16Be | Self::GrayAlpha16Be | Self::Rgb16Be | Self::Rgba16Be => 16,
        }
    }

    /// Bytes per pixel in the flat buffer.
    pub(crate) fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::Gray1
            | Self::Gray2
            | Self::Gray4
            | Self::Gray8
            | Self::Indexed1 { .. }
            | Self::Indexed2 { .. }
            | Self::Indexed4 { .. }
            | Self::Indexed8 { .. } => 1,
            Self::Gray16Be | Self::GrayAlpha8 => 2,
            Self::Rgb8 => 3,
            Self::GrayAlpha16Be | Self::Rgba8 => 4,
            Self::Rgb16Be => 6,
            Self::Rgba16Be => 8,
        }
    }

    /// Expected byte length of the pixel data buffer for the given dimensions.
    ///
    /// # Panics
    ///
    /// Panics if `width * height * bytes_per_pixel` overflows `usize`.
    pub(crate) fn data_len(&self, width: u32, height: u32) -> usize {
        (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(self.bytes_per_pixel()))
            .expect("pixel buffer size overflow")
    }
}
