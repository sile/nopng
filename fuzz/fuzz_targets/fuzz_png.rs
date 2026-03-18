#![no_main]

use libfuzzer_sys::fuzz_target;
use nopng::PngImage;

fuzz_target!(|data: &[u8]| {
    if let Ok(image) = PngImage::from_bytes(data) {
        let _ = image.width();
        let _ = image.height();
        let _ = image.data();

        if let Ok(encoded) = image.to_bytes() {
            let _ = PngImage::from_bytes(&encoded);
        }
    }
});
