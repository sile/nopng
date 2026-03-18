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

Decoded images are returned as RGBA8 via `PngRgbaImage`.

Supported Encoding
------------------

- Source image type: `PngRgbaImage` (`RGBA8`)
- `write_to()`:
  - chooses a PNG representation automatically
  - may emit grayscale, grayscale+alpha, rgb, rgba, or indexed-color PNG
- `write_to_with_options()`:
  - can force `grayscale`, `grayscale+alpha`, `rgb`, `rgba`, or `indexed-color`
  - can enable Adam7 interlacing
- Bit depth selection:
  - grayscale: `1/2/4/8-bit` when exactly representable
  - indexed-color: `1/2/4/8-bit`
  - other color types: `8-bit`

Example
-------

```rust
use nopng::{PngColorMode, PngEncodeOptions, PngRgbaImage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("image.png")?;
    let image = PngRgbaImage::from_bytes(&bytes)?;

    println!("{}x{}", image.width(), image.height());
    println!("rgba bytes: {}", image.data().len());

    let mut encoded = Vec::new();
    image.write_to_with_options(
        &mut encoded,
        PngEncodeOptions {
            color_mode: PngColorMode::Indexed,
            interlaced: true,
        },
    )?;
    Ok(())
}
```

TODO
----

- Animated PNG
- Encoding with compression
