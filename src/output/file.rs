use anyhow::Result;
use std::path::Path;

use crate::capture::screen::ScreenBitmap;

pub fn save_png(bmp: &ScreenBitmap, path: &Path) -> Result<()> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if matches!(ext.as_str(), "jpg" | "jpeg" | "jfif") {
        // JPEG 不支援 alpha，轉成 RGB
        let mut rgb = vec![0u8; (bmp.width * bmp.height * 3) as usize];
        for (i, chunk) in bmp.data.chunks_exact(4).enumerate() {
            rgb[i * 3]     = chunk[2]; // R
            rgb[i * 3 + 1] = chunk[1]; // G
            rgb[i * 3 + 2] = chunk[0]; // B
        }
        let img = image::RgbImage::from_raw(bmp.width as u32, bmp.height as u32, rgb)
            .ok_or_else(|| anyhow::anyhow!("failed to build image"))?;
        img.save(path)?;
    } else {
        let mut rgba = vec![0u8; bmp.data.len()];
        for (i, chunk) in bmp.data.chunks_exact(4).enumerate() {
            rgba[i * 4]     = chunk[2]; // R
            rgba[i * 4 + 1] = chunk[1]; // G
            rgba[i * 4 + 2] = chunk[0]; // B
            rgba[i * 4 + 3] = 255;      // A
        }
        let img = image::RgbaImage::from_raw(bmp.width as u32, bmp.height as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("failed to build image"))?;
        img.save(path)?;
    }
    Ok(())
}
