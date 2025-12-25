use image::{ImageFormat, RgbaImage};
use std::io::Cursor;

/// Encode an RgbaImage to PNG bytes
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, image::ImageError> {
    let mut buffer = Vec::new();
    let mut cursor = Cursor::new(&mut buffer);

    image.write_to(&mut cursor, ImageFormat::Png)?;

    Ok(buffer)
}
