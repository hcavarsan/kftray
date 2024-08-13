use std::fs::read_to_string;
use std::io;
use std::path::Path;

pub fn get_file_content(path: &Path) -> io::Result<String> {
    let mut content = String::new();

    if path.is_file() {
        content = read_to_string(path)?;
    }

    Ok(content)
}
