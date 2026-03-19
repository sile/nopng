#![no_std]
#![warn(missing_docs)]

//! `nopng` — a minimal, `no_std` PNG encoder/decoder.
//!
//! See [`decode_image`], [`encode_image`], [`inspect_image`], and [`ImageSpec`] for the main API.
//!
//! # Examples
//!
//! Encode a 2x2 RGBA8 image:
//!
//! ```
//! let spec = nopng::ImageSpec::new(2, 2, nopng::PixelFormat::Rgba8);
//! let pixels = vec![255u8; spec.data_len()]; // white, fully opaque
//! let png_bytes = nopng::encode_image(&spec, &pixels)?;
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Decode a PNG and convert to RGBA8:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
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
//! #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
//! #     &[255, 0, 0, 255],
//! # )?;
//! let (spec, pixels) = nopng::decode_image(&png_bytes)?;
//! match spec.pixel_format {
//!     nopng::PixelFormat::Rgba8 => { /* 4 bytes per pixel */ }
//!     nopng::PixelFormat::Gray8 => { /* 1 byte per pixel */ }
//!     _ => { /* ... */ }
//! }
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Pre-inspect a PNG to learn its format before decoding:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Gray8),
//! #     &[128],
//! # )?;
//! let spec = nopng::inspect_image(&png_bytes)?;
//! println!("{}x{}", spec.width, spec.height);
//! # Ok::<(), nopng::Error>(())
//! ```

extern crate alloc;

mod adler32;
mod chunk;
mod crc;
mod deflate;
mod pixel_reformat;
mod png;
mod png_decode;
mod png_encode;
mod png_types;
mod zlib;

pub use png::Error;
pub use png::ImageSpec;
pub use png::decode_image;
pub use png::encode_image;
pub use png::inspect_image;
pub use png::reformat_pixels;
pub use png_types::PixelFormat;
