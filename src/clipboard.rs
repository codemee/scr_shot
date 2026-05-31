use image::RgbaImage;

pub fn copy_image(img: &RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;

    let (width, height) = img.dimensions();
    let bgra: Vec<u8> = img
        .pixels()
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();

    let img_data = arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(bgra),
    };

    clipboard.set_image(img_data).map_err(|e| e.to_string())
}
