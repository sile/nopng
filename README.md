nopng
=====

[![nopng](https://img.shields.io/crates/v/nopng.svg)](https://crates.io/crates/nopng)
[![Documentation](https://docs.rs/nopng/badge.svg)](https://docs.rs/nopng)
[![Actions Status](https://github.com/sile/nopng/workflows/CI/badge.svg)](https://github.com/sile/nopng/actions)
![License](https://img.shields.io/crates/l/nopng)

A Rust [PNG] library with no dependencies, `no_std`, and no trait-based public API.

The `no` in `nopng` stands for:

- no dependencies
- no_std
- no trait-based public API

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
- `to_bytes()` uses `image.encoding()` as-is
- `PngEncoding::infer_from_rgba()` provides the same automatic selection used by `PngImage::new()`
- Bit depth selection:
  - grayscale: `1/2/4/8-bit` when exactly representable
  - indexed-color: `1/2/4/8-bit`
  - other color types: `8-bit`
- `PngBitDepth::Sixteen` may appear after decoding a 16-bit PNG, but `PngImage::to_bytes()` still writes an 8-bit PNG because `PngImage` stores RGBA8 pixels

no_std
------

- The library itself is `no_std` and uses `alloc`
- Decoding is byte-based via `PngImage::from_bytes(&[u8])`
- Encoding is byte-based via `PngImage::to_bytes()`
- `std` is only needed by callers that want file or console I/O

Example
-------

```rust
fn convert(bytes: &[u8]) -> nopng::Result<Vec<u8>> {
    let mut image = nopng::PngImage::from_bytes(bytes)?;
    *image.encoding_mut() = nopng::PngEncoding {
        color_mode: nopng::PngColorMode::Indexed,
        bit_depth: nopng::PngBitDepth::Four,
        interlaced: true,
    };
    image.to_bytes()
}
```

TODO
----

- Animated PNG
