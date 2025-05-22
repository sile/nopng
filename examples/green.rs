use nopng::PngRgbaImage;

fn main() -> std::io::Result<()> {
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

    // Create the PNG image
    let png_image = PngRgbaImage::new(width, height, data).expect("infallible");

    // Write the PNG to stdout
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    png_image.write_to(&mut stdout)?;

    Ok(())
}
