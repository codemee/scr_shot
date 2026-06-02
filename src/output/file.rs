use anyhow::Result;
use std::path::Path;

use crate::capture::screen::ScreenBitmap;

pub fn save_png(bmp: &ScreenBitmap, path: &Path) -> Result<()> {
    // Convert BGRA → RGBA for the image crate
    let mut rgba = vec![0u8; bmp.data.len()];
    for (i, chunk) in bmp.data.chunks_exact(4).enumerate() {
        rgba[i * 4]     = chunk[2]; // R
        rgba[i * 4 + 1] = chunk[1]; // G
        rgba[i * 4 + 2] = chunk[0]; // B
        rgba[i * 4 + 3] = 255;      // A
    }

    let img: image::RgbaImage = image::RgbaImage::from_raw(
        bmp.width as u32,
        bmp.height as u32,
        rgba,
    )
    .ok_or_else(|| anyhow::anyhow!("failed to build image"))?;

    img.save(path)?;
    Ok(())
}
