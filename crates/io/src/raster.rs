//! Reading a picture file (PNG, JPEG, TIFF) as grey pixels, for `import`. Transparent pixels count as paper (white).

use image::{ImageReader, imageops::FilterType};

/// Most pixels read from one picture (larger ones are refused, not reduced, so a bad file cannot take the memory).
const MAX_PIXELS: u64 = 120_000_000;

pub struct Gray {
    pub width: usize,
    pub height: usize,
    /// Row 0 on top.
    pub gray: Vec<u8>,
}

/// Decode `bytes` (the format is found from the content) to 8-bit grey.
pub fn read_gray(bytes: &[u8]) -> Result<Gray, String> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| format!("not a PNG, JPEG or TIFF picture ({e})"))?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("the picture is {w} x {h} pixels: too large to read"));
    }
    let img = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?;
    // alpha onto white, then grey
    let rgba = img.to_rgba8();
    let gray: Vec<u8> = rgba
        .pixels()
        .map(|p| {
            let a = u32::from(p[3]);
            let g = (u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000;
            ((g * a + 255 * (255 - a)) / 255) as u8
        })
        .collect();
    let _ = FilterType::Nearest;
    Ok(Gray { width: rgba.width() as usize, height: rgba.height() as usize, gray })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_with_a_dark_square_reads_as_grey() {
        let mut img = image::RgbaImage::from_pixel(4, 3, image::Rgba([255, 255, 255, 255]));
        img.put_pixel(1, 1, image::Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 1, image::Rgba([0, 0, 0, 0])); // transparent: paper
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let g = read_gray(&png).unwrap();
        assert_eq!((g.width, g.height), (4, 3));
        assert_eq!(g.gray[1 * 4 + 1], 0);
        assert_eq!(g.gray[1 * 4 + 2], 255);
        assert!(read_gray(b"not a picture").is_err());
    }

    #[test]
    fn a_tiff_is_read_too() {
        let img = image::GrayImage::from_pixel(5, 2, image::Luma([10]));
        let mut tiff = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut tiff), image::ImageFormat::Tiff).unwrap();
        let g = read_gray(&tiff).unwrap();
        assert_eq!((g.width, g.height, g.gray[0]), (5, 2, 10));
    }
}
