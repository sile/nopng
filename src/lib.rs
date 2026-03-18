#![no_std]

extern crate alloc;

mod adler32;
mod chunk;
mod crc;
mod deflate;
mod png;
mod zlib;

pub use png::Error;
pub use png::PngBitDepth;
pub use png::PngColorMode;
pub use png::PngEncoding;
pub use png::PngImage;
pub use png::PngInfo;
pub use png::Result;
