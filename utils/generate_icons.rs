use image::io::Reader as ImageReader;
use image::{imageops::FilterType, DynamicImage};
use std::fs::{self, File};
use std::path::Path;

const SRC_FILE: &str = "./img/logo.png";
const DST_PATH: &str = "./src-tauri/icons";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(DST_PATH)?;

    println!("Info: Generating Icons");

    generate_png_icons()?;
    generate_icns()?;
    generate_ico()?;

    println!("Info: Done generating icons.");
    Ok(())
}

fn generate_png_icons() -> Result<(), Box<dyn std::error::Error>> {
    println!("Info: Generating PNG icons ...");
    let sizes = [32, 128, 256, 512];
    let src = ImageReader::open(SRC_FILE)?.decode()?;
    for size in sizes.iter() {
        resize_and_save(
            &src,
            *size,
            *size,
            &format!("{}/{}x{}.png", DST_PATH, size, size),
        )?;
    }
    resize_and_save(&src, 256, 256, &format!("{}/128x128@2x.png", DST_PATH))?;
    Ok(())
}

fn generate_icns() -> Result<(), Box<dyn std::error::Error>> {
    println!("Info: Generating icon.icns ...");
    let icns_path = Path::new(DST_PATH).join("icon.iconset");
    fs::create_dir_all(&icns_path)?;

    let sizes = [16, 32, 128, 256, 512, 1024];
    let src = ImageReader::open(SRC_FILE)?.decode()?;
    for size in sizes.iter() {
        let factor = if *size > 16 { 2 } else { 1 };
        let actual_size = size / factor;
        let file_name = if factor > 1 {
            format!("icon_{}x{}@{}x.png", actual_size, actual_size, factor)
        } else {
            format!("icon_{}x{}.png", size, size)
        };
        resize_and_save(
            &src,
            *size,
            *size,
            icns_path.join(file_name).to_str().unwrap(),
        )?;
    }

    std::process::Command::new("iconutil")
        .args([
            "-c",
            "icns",
            icns_path.to_str().unwrap(),
            "-o",
            &format!("{}/icon.icns", DST_PATH),
        ])
        .spawn()?
        .wait()?;

    fs::remove_dir_all(icns_path)?;
    Ok(())
}

fn generate_ico() -> Result<(), Box<dyn std::error::Error>> {
    println!("Info: Generating icon.ico ...");

    let src = ImageReader::open(SRC_FILE)?.decode()?;
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    let sizes = [16, 24, 32, 48, 64, 256, 512];

    for size in sizes.iter() {
        let resized = src.resize_exact(*size, *size, FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        let (width, height) = rgba.dimensions();
        let ico_image = ico::IconImage::from_rgba_data(width, height, rgba.into_raw());
        icon_dir.add_entry(ico::IconDirEntry::encode(&ico_image)?);
    }

    let file_out = File::create(format!("{}/icon.ico", DST_PATH))?;
    icon_dir.write(file_out)?;

    Ok(())
}

fn resize_and_save(
    src: &DynamicImage,
    width: u32,
    height: u32,
    save_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    src.resize(width, height, FilterType::Lanczos3)
        .save(save_path)?;
    Ok(())
}
