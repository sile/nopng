#![no_main]

use libfuzzer_sys::fuzz_target;
use nopng::PngRgbaImage;

fuzz_target!(|data: &[u8]| {
    if let Ok(image) = PngRgbaImage::from_bytes(data) {
        let _ = image.width();
        let _ = image.height();
        let _ = image.data();

        let mut encoded = Vec::new();
        if image.write_to(&mut encoded).is_ok() {
            let _ = PngRgbaImage::from_bytes(&encoded);
        }
    }
});
