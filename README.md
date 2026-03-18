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

Decoded images are returned as `PngImage<'static>`.
`PngImage` stores a `PngPixels` enum that preserves the source layout as closely as possible:

- low-bit grayscale is returned as unpacked `Gray { bit_depth, samples }`
- indexed PNG is returned as `Indexed { bit_depth, indices, palette, trns }`
- `tRNS` is reflected in the pixel representation
- 16-bit PNG is returned as native `u16`-backed pixel data

Use `nopng::PngInfo::from_bytes()` if you want to inspect width, height, bit depth, or interlace mode before doing a full decode.

Supported Encoding
------------------

- `PngImage<'a>` can hold borrowed or owned pixel data
- Pixel storage is represented by `PngPixels<'a>`
- `PngImage` stores a concrete `PngEncoding`
- `to_bytes()` uses `image.encoding()` as-is
- `PngEncoding::for_pixels()` provides a natural default for a given `PngPixels`
- `to_bytes()` performs implicit pixel conversion when `pixels` and `encoding` do not exactly match

no_std
------

- The library itself is `no_std` and uses `alloc`
- `nopng::PngInfo::from_bytes(&[u8])` reads only the PNG signature and `IHDR`
- Decoding is byte-based via `PngImage::from_bytes(&[u8])`
- Encoding is byte-based via `PngImage::to_bytes()`
- `std` is only needed by callers that want file or console I/O

Example
-------

```rust
fn convert(bytes: &[u8]) -> nopng::Result<Vec<u8>> {
    let info = nopng::PngInfo::from_bytes(bytes)?;
    if info.decoded_rgba8_bytes().unwrap_or(usize::MAX) > 16 * 1024 * 1024 {
        return Err(nopng::Error::InvalidData("image is too large".into()));
    }

    let mut image = nopng::PngImage::from_bytes(bytes)?;
    *image.encoding_mut() = nopng::PngEncoding {
        color_mode: nopng::PngColorMode::Indexed,
        bit_depth: nopng::PngBitDepth::Four,
        interlaced: true,
    };
    image.to_bytes()
}
```

```rust
fn encode_borrowed_rgb(data: &[u8]) -> nopng::Result<Vec<u8>> {
    let pixels = nopng::PngPixels::infer_from_rgb8(data);
    let image = nopng::PngImage::new(
        2,
        1,
        pixels,
        nopng::PngEncoding {
            color_mode: nopng::PngColorMode::Rgb,
            bit_depth: nopng::PngBitDepth::Eight,
            interlaced: false,
        },
    )?;
    image.to_bytes()
}
```

TODO
----

- Streaming / incremental API
- Animated PNG
