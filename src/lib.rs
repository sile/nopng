#![no_std]

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
pub use png_types::PixelFormat;
