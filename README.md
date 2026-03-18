nopng
=====

[![nopng](https://img.shields.io/crates/v/nopng.svg)](https://crates.io/crates/nopng)
[![Documentation](https://docs.rs/nopng/badge.svg)](https://docs.rs/nopng)
[![Actions Status](https://github.com/sile/nopng/workflows/CI/badge.svg)](https://github.com/sile/nopng/actions)
![License](https://img.shields.io/crates/l/nopng)

A Rust [PNG] library with no dependencies.

[PNG]: https://www.w3.org/TR/PNG/

Supported Decoding
------------------

- Non-interlaced PNG
- Adam7 interlaced PNG
- Color types:
  - Grayscale: 1/2/4/8/16-bit
  - Truecolor: 8/16-bit
  - Indexed-color: 1/2/4/8-bit
  - Grayscale with alpha: 8/16-bit
  - Truecolor with alpha: 8/16-bit
- `PLTE` and `tRNS`

Decoded images are returned as RGBA8 via `PngImage`.
If the source PNG is 16-bit, samples are downconverted to 8-bit when stored in `PngImage`.

Supported Encoding
------------------

- Source image type: `PngImage` (`RGBA8`)
- `PngImage` stores a concrete `PngEncoding`
- `write_to()` uses `image.encoding()` as-is
- `PngEncoding::infer_from_rgba()` provides the same automatic selection used by `PngImage::new()`
- Bit depth selection:
  - grayscale: `1/2/4/8-bit` when exactly representable
  - indexed-color: `1/2/4/8-bit`
  - other color types: `8-bit`
- `PngBitDepth::Sixteen` may appear after decoding a 16-bit PNG, but `PngImage::write_to()` still writes an 8-bit PNG because `PngImage` stores RGBA8 pixels

Example
-------

```rust
use nopng::{PngBitDepth, PngColorMode, PngEncoding, PngImage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("image.png")?;
    let mut image = PngImage::from_bytes(&bytes)?;

    println!("{}x{}", image.width(), image.height());
    println!("rgba bytes: {}", image.data().len());
    println!("source encoding: {:?}", image.encoding());

    *image.encoding_mut() = PngEncoding {
        color_mode: PngColorMode::Indexed,
        bit_depth: PngBitDepth::Four,
        interlaced: true,
    };
    let mut encoded = Vec::new();
    image.write_to(&mut encoded)?;
    Ok(())
}
```

TODO
----

- Animated PNG
