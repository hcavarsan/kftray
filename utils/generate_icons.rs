use image::io::Reader as ImageReader;
use image::{DynamicImage, ImageError, ImageFormat};
use std::fs::{self, File};
use std::io::Error as IoError;
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

const SRC_FILE: &str = "./img/logo.png";
const DST_PATH: &str = "./src-tauri/icons";

#[derive(Debug, Error)]
enum CustomError {
    #[error("Image error: {0}")]
    ImageError(#[from] ImageError),
    #[error("IO error: {0}")]
    IoError(#[from] IoError),
}

fn main() -> Result<(), CustomError> {
    fs::create_dir_all(DST_PATH)?;

    println!("Info: Generating Icons");

    generate_png_icons()?;
    generate_icns()?;
    generate_ico()?;

    println!("Info: Done generating icons.");
    Ok(())
}

fn generate_png_icons() -> Result<(), CustomError> {
    println!("Info: Generating PNG icons ...");
    let sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024];
    let src = ImageReader::open(SRC_FILE)?
        .with_guessed_format()?
        .decode()?;
    for size in sizes.iter() {
        resize_and_save(
            &src,
            *size,
            *size,
            &PathBuf::from(DST_PATH).join(format!("icon_{}x{}.png", size, size)),
        )?;
    }
    Ok(())
}

fn generate_icns() -> Result<(), CustomError> {
    println!("Info: Generating icon.icns ...");
    let icns_path = PathBuf::from(DST_PATH).join("icon.iconset");
    fs::create_dir_all(&icns_path)?;

    let sizes = [16, 32, 64, 128, 256, 512, 1024];
    let src = ImageReader::open(SRC_FILE)?.decode()?;
    for size in sizes.iter() {
        let factor = if *size > 16 { 2 } else { 1 };
        let actual_size = size / factor;
        let file_name = if factor > 1 {
            format!("icon_{}x{}@2x.png", actual_size, actual_size)
        } else {
            format!("icon_{}x{}.png", size, size)
        };
        resize_and_save(&src, *size, *size, &icns_path.join(&file_name))?;
    }

    Command::new("iconutil")
        .arg("-c")
        .arg("icns")
        .arg(&icns_path)
        .arg("-o")
        .arg(PathBuf::from(DST_PATH).join("icon.icns"))
        .status()?;

    fs::remove_dir_all(icns_path)?;
    Ok(())
}

fn generate_ico() -> Result<(), CustomError> {
    println!("Info: Generating icon.ico ...");

    let src = ImageReader::open(SRC_FILE)?.decode()?;
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    let sizes = [16, 32, 48, 64, 128, 256];

    for &size in &sizes {
        let resized = src.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        let (width, height) = rgba.dimensions();
        let ico_image = ico::IconImage::from_rgba_data(width, height, rgba.into_raw());
        icon_dir.add_entry(ico::IconDirEntry::encode(&ico_image)?);
    }

    let file_path = PathBuf::from(DST_PATH).join("icon.ico");
    let file_out = File::create(file_path)?;
    icon_dir.write(file_out)?;

    Ok(())
}

fn resize_and_save(
    src: &DynamicImage,
    width: u32,
    height: u32,
    save_path: &PathBuf,
) -> Result<(), CustomError> {
    src.resize(width, height, image::imageops::FilterType::Lanczos3)
        .save_with_format(save_path, ImageFormat::Png)?;
    Ok(())
}
