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
- Animated PNG (APNG): decode to full-canvas composited frames, encode full-canvas frames
- `inspect` for reading dimensions, format, frame count, and interlace before decoding
- `reformat_pixels` for pixel format conversion without a full encode/decode round-trip

Examples
--------

### Encode a 2x2 RGBA8 image

```rust
fn main() -> Result<(), nopng::Error> {
    let spec = nopng::ImageSpec {
        width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        pixel_format: nopng::PixelFormat::Rgba8,
    };
    let pixels = vec![255u8; spec.data_len()]; // white, fully opaque
    let png_bytes = nopng::encode_image(&spec, &pixels)?;
    Ok(())
}
```

### Decode and reformat to RGBA8

```rust
fn main() -> Result<(), nopng::Error> {
    let spec = nopng::ImageSpec {
        width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        pixel_format: nopng::PixelFormat::Rgba8,
    };
    let pixels = vec![255u8; spec.data_len()];
    let png_bytes = nopng::encode_image(&spec, &pixels)?;

    let (spec, pixels) = nopng::decode_image(&png_bytes)?;
    let rgba = nopng::reformat_pixels(&spec.pixel_format, &pixels, &nopng::PixelFormat::Rgba8)?;
    Ok(())
}
```

### Encode an indexed (palette) image

```rust
fn main() -> Result<(), nopng::Error> {
    let palette = vec![
        255, 0, 0,   // index 0: red
        0, 255, 0,   // index 1: green
        0, 0, 255,   // index 2: blue
        255, 255, 0, // index 3: yellow
    ];
    let spec = nopng::ImageSpec {
        width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        pixel_format: nopng::PixelFormat::Indexed8 { palette, trns: None },
    };
    let indices = vec![0, 1, 2, 3]; // one index per pixel
    let png_bytes = nopng::encode_image(&spec, &indices)?;
    Ok(())
}
```

### Convert pixel data between formats

```rust
fn main() -> Result<(), nopng::Error> {
    // RGB8 pixels (3 bytes each)
    let rgb = vec![255, 0, 0, 0, 255, 0]; // red, green
    let rgba = nopng::reformat_pixels(
        &nopng::PixelFormat::Rgb8,
        &rgb,
        &nopng::PixelFormat::Rgba8,
    )?;
    assert_eq!(rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
    Ok(())
}
```

### Encode an animation and decode it again

```rust
fn main() -> Result<(), nopng::Error> {
    let animation = nopng::Animation {
        width: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        height: std::num::NonZeroU32::new(2).expect("2 is non-zero"),
        pixel_format: nopng::PixelFormat::Rgba8,
        play_count: nopng::PlayCount::Infinite,
        frames: vec![nopng::Frame {
            pixels: vec![255, 0, 0, 255].repeat(4),
            delay: nopng::Delay {
                num: 1,
                den: std::num::NonZeroU16::new(10).expect("10 is non-zero"),
            },
        }],
    };
    let bytes = nopng::encode_animation(&animation)?;
    let decoded = nopng::decode_animation(&bytes)?;
    assert_eq!(decoded.frames.len(), 1);
    Ok(())
}
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

APNG
----

nopng can read and write animated PNGs. An animation is a canvas size, a
pixel format, a play count, and a list of full-canvas frames. The
`acTL`/`fcTL`/`fdAT` bookkeeping (frame rectangles, dispose/blend
operations, sequence numbers) is handled internally and is not exposed.

| Property                   | Decode                         | Encode                          |
|----------------------------|--------------------------------|---------------------------------|
| Pixel format               | always `Rgba16Be` (composited) | any format from the table above |
| Frame size                 | expanded to the full canvas    | always the full canvas          |
| Dispose / blend operations | applied, not exposed           | always `NONE` / `SOURCE`        |
| Interlace                  | read from the file             | always non-interlaced           |

Decoded frames are composited, so each one is a complete image that can be
displayed without consulting its neighbours. The output format is always
`Rgba16Be` — the lossless common denominator for mixing frames — so use
`reformat_pixels` to narrow it if needed.
