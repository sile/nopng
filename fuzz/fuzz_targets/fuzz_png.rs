#![no_main]

use libfuzzer_sys::fuzz_target;
use nopng::{PixelFormat, decode_image, encode_image, reformat_pixels};

const REFORMAT_TARGETS: &[PixelFormat] = &[
    PixelFormat::Gray8,
    PixelFormat::Gray16Be,
    PixelFormat::GrayAlpha8,
    PixelFormat::GrayAlpha16Be,
    PixelFormat::Rgb8,
    PixelFormat::Rgb16Be,
    PixelFormat::Rgba8,
    PixelFormat::Rgba16Be,
];

fuzz_target!(|data: &[u8]| {
    if let Ok((spec, pixels)) = decode_image(data) {
        let _ = spec.width;
        let _ = spec.height;

        // reformat_pixels to every non-indexed format must not panic.
        for dst in REFORMAT_TARGETS {
            let _ = reformat_pixels(&spec.pixel_format, &pixels, dst);
        }

        if let Ok(encoded) = encode_image(&spec, &pixels) {
            let _ = decode_image(&encoded);
        }
    }
});
