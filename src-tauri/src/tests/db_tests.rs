use std::fs;
use std::path::Path;
use tempfile::{tempdir, NamedTempFile};

use crate::db::{init, db_file_exists, create_db_file, get_db_path};
#[test]
fn test_get_db_path() {
    let expected_end = "/.kftray/configs.db";
    assert!(get_db_path().ends_with(expected_end));
}
#[test]
fn test_db_file_exists() {
    let db_path = get_db_path();
    fs::File::create(&db_path).unwrap();
    assert_eq!(db_file_exists(), true);
}
#[test]
fn test_create_db_file() {
    let db_path = get_db_path();
    let db_dir = Path::new(&db_path).parent().unwrap();
    if db_dir.exists() {
        fs::remove_dir_all(db_dir).unwrap();
    }
    create_db_file();
    assert_eq!(Path::new(&db_path).exists(), true);
}
#[test]
fn test_init() {
    let db_path = get_db_path();
    let db_dir = Path::new(&db_path).parent().unwrap();
    if db_dir.exists() {
        fs::remove_dir_all(db_dir).unwrap();
    }
    init();
    assert_eq!(Path::new(&db_path).exists(), true);
}



