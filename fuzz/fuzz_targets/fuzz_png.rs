#![no_main]

use libfuzzer_sys::fuzz_target;
use nopng::{encode_image, decode_image};

fuzz_target!(|data: &[u8]| {
    if let Ok((spec, pixels)) = decode_image(data) {
        let _ = spec.width;
        let _ = spec.height;

        if let Ok(encoded) = encode_image(&spec, &pixels) {
            let _ = decode_image(&encoded);
        }
    }
});
