#![no_std]
#![warn(missing_docs)]

//! `nopng` — a minimal, `no_std` PNG encoder/decoder.
//!
//! See [`decode_image`], [`encode_image`], and [`ImageSpec`] for the main API.
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
//! Decode a PNG into RGBA8 pixels:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
//! #     &[255, 0, 0, 255],
//! # )?;
//! let (spec, pixels) = nopng::decode_image(&png_bytes, Some(&nopng::PixelFormat::Rgba8))?;
//! assert_eq!(pixels.len(), spec.data_len());
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
//! let (spec, pixels) = nopng::decode_image(&png_bytes, None)?;
//! match spec.pixel_format {
//!     nopng::PixelFormat::Rgba8 => { /* 4 bytes per pixel */ }
//!     nopng::PixelFormat::Gray8 => { /* 1 byte per pixel */ }
//!     _ => { /* ... */ }
//! }
//! # Ok::<(), nopng::Error>(())
//! ```
//!
//! Pre-allocate the output buffer with [`decode_image_into`]:
//!
//! ```
//! # let png_bytes = nopng::encode_image(
//! #     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Gray8),
//! #     &[128],
//! # )?;
//! let spec = nopng::inspect_image(&png_bytes)?;
//! let mut buf = vec![0u8; spec.data_len()];
//! nopng::decode_image_into(&png_bytes, None, &mut buf)?;
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
pub use png::Result;
pub use png::decode_image;
pub use png::decode_image_into;
pub use png::encode_image;
pub use png::inspect_image;
pub use png_types::PixelFormat;
