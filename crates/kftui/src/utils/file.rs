use std::fs::read_to_string;
use std::io;
use std::path::Path;

pub fn get_file_content(path: &Path) -> io::Result<String> {
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Path is not a file",
        ));
    }

    let content = read_to_string(path)?;
    Ok(content)
}
