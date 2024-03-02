use image::io::Reader as ImageReader;
use image::{DynamicImage, ImageError, ImageFormat};
use std::fs::{self, File};
use std::io::{BufWriter, Error as IoError};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

const SRC_FILE: &str = "./img/logo.png";
const DST_PATH: &str = "./src-tauri/icons";

#[derive(Debug, Error)]
pub enum CustomError {
    #[error("Image error: {0}")]
    ImageError(#[from] ImageError),

    #[error("IO error: {0}")]
    IoError(#[from] IoError),

    #[error("Iconutil command failed: {0}")]
    CommandError(String),
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
    let sizes = [32, 128];
    let src = ImageReader::open(SRC_FILE)?
        .with_guessed_format()?
        .decode()?;

    resize_and_save(
        &src,
        256,
        256,
        &PathBuf::from(DST_PATH).join("128x128@2x.png"),
    )?;

    for size in sizes.iter() {
        resize_and_save(
            &src,
            *size,
            *size,
            &PathBuf::from(DST_PATH).join(format!("{}x{}.png", size, size)),
        )?;
    }
    Ok(())
}
fn generate_icns() -> Result<(), CustomError> {
    println!("Info: Generating icon.icns ...");

    let icns_path = PathBuf::from(DST_PATH).join("icon.iconset");
    fs::create_dir_all(&icns_path)?;

    let sizes = [
        (16, 1),
        (16, 2), // 16x16@1x and 16x16@2x (32x32)
        (32, 1),
        (32, 2), // 32x32@1x and 32x32@2x (64x64)
        (128, 1),
    ];

    let src = ImageReader::open(SRC_FILE)?.decode()?;

    for (size, factor) in sizes {
        let filename = if factor == 2 {
            format!("icon_{}x{}@2x.png", size / factor, size / factor)
        } else {
            format!("icon_{}x{}.png", size, size)
        };
        let output_size = size * factor;
        resize_and_save(&src, output_size, output_size, &icns_path.join(&filename))?;
    }

    let output = Command::new("iconutil")
        .args(["-c", "icns", &icns_path.to_str().unwrap(), "-o"])
        .arg(icns_path.with_extension("icns").to_str().unwrap())
        .output()?;

    if !output.status.success() {
        let error_message = String::from_utf8_lossy(&output.stderr).to_string();
        fs::remove_dir_all(&icns_path)?;
        return Err(CustomError::CommandError(error_message));
    }

    fs::remove_dir_all(&icns_path)?;
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
    let file_out = BufWriter::new(File::create(file_path)?);
    icon_dir.write(file_out)?;

    Ok(())
}

fn resize_and_save(
    src: &DynamicImage,
    width: u32,
    height: u32,
    save_path: &PathBuf,
) -> Result<(), CustomError> {
    src.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
        .save_with_format(save_path, ImageFormat::Png)?;
    Ok(())
}
