#![no_std]

extern crate alloc;

mod adler32;
mod chunk;
mod crc;
mod deflate;
mod png;
mod png_decode;
mod png_encode;
mod png_pixels;
mod png_types;
mod zlib;

pub use png::Error;
pub use png::ImageSpec;
pub use png::Result;
pub use png::decode_image;
pub use png::encode_image;
pub use png_pixels::Pixels;
pub use png_types::BitDepth;
pub use png_types::ColorMode;
