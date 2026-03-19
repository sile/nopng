use std::io::Write;

use nopng::{ImageSpec, PixelFormat, encode_image};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a 32x32 RGBA image (4 bytes per pixel)
    let width = 32;
    let height = 32;

    // Create the image data - solid green (R=0, G=255, B=0, A=255)
    // Format is RGBA, so each pixel has 4 bytes
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        data.push(0); // Red = 0
        data.push(255); // Green = 255
        data.push(0); // Blue = 0
        data.push(255); // Alpha = 255 (fully opaque)
    }

    // Create the image spec and encode
    let spec = ImageSpec {
        width,
        height,
        pixel_format: PixelFormat::Rgba8,
        interlaced: false,
    };
    let bytes = encode_image(&spec, &data)?;

    // Write the PNG to stdout
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(&bytes)?;

    Ok(())
}
