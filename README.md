nopng
=====

[![nopng](https://img.shields.io/crates/v/nopng.svg)](https://crates.io/crates/nopng)
[![Documentation](https://docs.rs/nopng/badge.svg)](https://docs.rs/nopng)
[![Actions Status](https://github.com/sile/nopng/workflows/CI/badge.svg)](https://github.com/sile/nopng/actions)
![License](https://img.shields.io/crates/l/nopng)

A minimal PNG encoder/decoder with no dependencies and `no_std`.

Features
--------

- No dependencies
- `no_std` (`alloc` only)
- Decode: all color types (grayscale, truecolor, indexed, with/without alpha, 1–16 bit), Adam7 interlace
- Encode: all color types, Adam7 interlace
- `reformat_pixels` for pixel format conversion without a full encode/decode round-trip

Examples
--------

### Encode a 2x2 RGBA8 image

```rust
let spec = nopng::ImageSpec::new(2, 2, nopng::PixelFormat::Rgba8);
let pixels = vec![255u8; spec.data_len()]; // white, fully opaque
let png_bytes = nopng::encode_image(&spec, &pixels)?;
# Ok::<(), nopng::Error>(())
```

### Decode and reformat to RGBA8

```rust
# let png_bytes = nopng::encode_image(
#     &nopng::ImageSpec::new(1, 1, nopng::PixelFormat::Rgba8),
#     &[255, 0, 0, 255],
# )?;
let (spec, pixels) = nopng::decode_image(&png_bytes)?;
let rgba = nopng::reformat_pixels(&spec.pixel_format, &pixels, &nopng::PixelFormat::Rgba8)?;
# Ok::<(), nopng::Error>(())
```

### Encode an indexed (palette) image

```rust
let palette = vec![
    255, 0, 0,   // index 0: red
    0, 255, 0,   // index 1: green
    0, 0, 255,   // index 2: blue
    255, 255, 0, // index 3: yellow
];
let spec = nopng::ImageSpec::new(
    2, 2,
    nopng::PixelFormat::Indexed8 { palette, trns: None },
);
let indices = vec![0, 1, 2, 3]; // one index per pixel
let png_bytes = nopng::encode_image(&spec, &indices)?;
# Ok::<(), nopng::Error>(())
```

### Convert pixel data between formats

```rust
// RGB8 pixels (3 bytes each)
let rgb = vec![255, 0, 0, 0, 255, 0]; // red, green
let rgba = nopng::reformat_pixels(
    &nopng::PixelFormat::Rgb8,
    &rgb,
    &nopng::PixelFormat::Rgba8,
)?;
assert_eq!(rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
# Ok::<(), nopng::Error>(())
```

Supported Formats
-----------------

| Color type             | Bit depths       | Decode | Encode |
|------------------------|------------------|--------|--------|
| Grayscale              | 1, 2, 4, 8, 16  | yes    | yes    |
| Grayscale + alpha      | 8, 16            | yes    | yes    |
| Truecolor (RGB)        | 8, 16            | yes    | yes    |
| Truecolor + alpha      | 8, 16            | yes    | yes    |
| Indexed (palette)      | 1, 2, 4, 8      | yes    | yes    |

Adam7 interlace is supported for both encoding and decoding.

Limitations
-----------

- Animated PNG (APNG) is not supported
