#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! `nopng` — a minimal, `no_std` PNG encoder/decoder.
//!
//! See [`decode_image`], [`encode_image`], [`inspect`], and [`ImageSpec`] for the main API.
//!
//! # Examples
//!
//! Encode a 2x2 RGBA8 image:
//!
//! ```
//! let spec = nopng::ImageSpec {
//!     width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
//!     height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
//!     pixel_format: nopng::PixelFormat::Rgba8,
//! };
//! let pixels = vec![255u8; spec.data_len()]; // white, fully opaque
//! let png_bytes = nopng::encode_image(&spec, &pixels)?;
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Convert decoded pixels to RGBA8 with `reformat_pixels`:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec {
//! #         width: std::num::NonZeroU32::MIN,
//! #         height: std::num::NonZeroU32::MIN,
//! #         pixel_format: nopng::PixelFormat::Rgba8,
//! #     },
//! #     &[255, 0, 0, 255],
//! # )?;
//! let (spec, pixels) = nopng::decode_image(&png_bytes)?;
//! let rgba = nopng::reformat_pixels(&spec.pixel_format, &pixels, &nopng::PixelFormat::Rgba8)?;
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Decode in the PNG's native format and inspect it:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec {
//! #         width: std::num::NonZeroU32::MIN,
//! #         height: std::num::NonZeroU32::MIN,
//! #         pixel_format: nopng::PixelFormat::Rgba8,
//! #     },
//! #     &[255, 0, 0, 255],
//! # )?;
//! let (spec, pixels) = nopng::decode_image(&png_bytes)?;
//! match spec.pixel_format {
//!     nopng::PixelFormat::Rgba8 => { /* 4 bytes per pixel */ }
//!     nopng::PixelFormat::Gray8 => { /* 1 byte per pixel */ }
//!     nopng::PixelFormat::Rgba16Be => { /* 8 bytes per pixel */ }
//!     _ => { /* alpha, 16-bit gray/RGB, or indexed */ }
//! }
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Pre-inspect a PNG to learn its format before decoding:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec {
//! #         width: std::num::NonZeroU32::MIN,
//! #         height: std::num::NonZeroU32::MIN,
//! #         pixel_format: nopng::PixelFormat::Gray8,
//! #     },
//! #     &[128],
//! # )?;
//! let info = nopng::inspect(&png_bytes)?;
//! println!("{}x{}", info.width, info.height);
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Inspect, decode, and conditionally reformat:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec {
//! #         width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
//! #         height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
//! #         pixel_format: nopng::PixelFormat::Gray8,
//! #     },
//! #     &[0, 64, 128, 255],
//! # )?;
//! let info = nopng::inspect(&png_bytes)?;
//! let (spec, pixels) = nopng::decode_image(&png_bytes)?;
//!
//! // Only reformat when the native format isn't already RGBA8.
//! let rgba = if spec.pixel_format == nopng::PixelFormat::Rgba8 {
//!     pixels
//! } else {
//!     nopng::reformat_pixels(&spec.pixel_format, &pixels, &nopng::PixelFormat::Rgba8)?
//! };
//! assert_eq!(rgba.len(), spec.width.get() as usize * spec.height.get() as usize * 4);
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Encode a 4-colour indexed (palette) image:
//!
//! ```
//! let palette = vec![
//!     255, 0, 0,   // index 0: red
//!     0, 255, 0,   // index 1: green
//!     0, 0, 255,   // index 2: blue
//!     255, 255, 0, // index 3: yellow
//! ];
//! let spec = nopng::ImageSpec {
//!     width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
//!     height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
//!     pixel_format: nopng::PixelFormat::Indexed8 { palette, trns: None },
//! };
//! let indices = vec![0, 1, 2, 3]; // one index per pixel
//! let png_bytes = nopng::encode_image(&spec, &indices)?;
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Convert pixel data between formats without encoding/decoding:
//!
//! ```
//! // RGB8 pixels (3 bytes each)
//! let rgb = vec![255, 0, 0, 0, 255, 0]; // red, green
//! let rgba = nopng::reformat_pixels(
//!     &nopng::PixelFormat::Rgb8,
//!     &rgb,
//!     &nopng::PixelFormat::Rgba8,
//! )?;
//! assert_eq!(rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
//! # Ok::<(), nopng::Error>(())
//! ```

extern crate alloc;

mod adler32;
mod animation;
mod apng;
mod apng_decode;
mod apng_encode;
mod chunk;
mod crc;
mod deflate;
mod pixel_reformat;
mod png;
mod png_decode;
mod png_encode;
mod png_types;
mod zlib;

pub use animation::{Animation, Delay, Frame, PlayCount, decode_animation, encode_animation};
pub use png::Error;
pub use png::ImageSpec;
pub use png::Inspection;
pub use png::decode_image;
pub use png::encode_image;
pub use png::encode_image_interlaced;
pub use png::inspect;
pub use png::reformat_pixels;
pub use png_types::PixelFormat;
