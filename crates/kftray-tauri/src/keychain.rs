use keyring::{
    Entry,
    Error as KeyringError,
};
use tauri::{
    Error as TauriError,
    InvokeError,
};

#[derive(Debug)]
pub enum CustomError {
    Keyring(KeyringError),
    Tauri(TauriError),
}

// Define a custom error type that encapsulates errors from different sources.
impl From<KeyringError> for CustomError {
    fn from(error: KeyringError) -> Self {
        CustomError::Keyring(error)
    }
}

// Implement conversion from `KeyringError` to `CustomError`.
impl From<TauriError> for CustomError {
    fn from(error: TauriError) -> Self {
        CustomError::Tauri(error)
    }
}

impl From<CustomError> for InvokeError {
    fn from(error: CustomError) -> Self {
        match error {
            CustomError::Keyring(err) => InvokeError::from(err.to_string()),
            CustomError::Tauri(err) => InvokeError::from(err.to_string()),
        }
    }
}

/// Stores a key using the `keyring` crate.
#[tauri::command]
pub fn store_key(
    service: &str, name: &str, password: &str,
) -> std::result::Result<(), CustomError> {
    let entry = Entry::new(service, name).map_err(CustomError::from)?;

    entry.set_password(password).map_err(CustomError::from)?;

    Ok(())
}

/// Retrieves a key using the `keyring` crate.
#[tauri::command]
pub fn get_key(service: &str, name: &str) -> std::result::Result<String, CustomError> {
    let entry = Entry::new(service, name).map_err(CustomError::from)?;

    let password = entry.get_password().map_err(CustomError::from)?;

    Ok(password)
}

/// Deletes a key using the `keyring` crate.
#[tauri::command]
pub fn delete_key(service: &str, name: &str) -> std::result::Result<(), CustomError> {
    let entry = Entry::new(service, name).map_err(CustomError::from)?;

    entry.delete_credential().map_err(CustomError::from)?;

    Ok(())
}

#[cfg(test)]

mod tests {

    use super::*;

    const SERVICE: &str = "test_service";

    const ACCOUNT: &str = "test_account";

    const PASSWORD: &str = "test_password";

    #[test]

    fn test_store_key_success() {
        let res = store_key(SERVICE, ACCOUNT, PASSWORD);

        assert!(res.is_ok());

        let entry = Entry::new(SERVICE, ACCOUNT).unwrap();

        let delete_result = entry.delete_credential();

        assert!(delete_result.is_ok());
    }

    #[test]

    fn test_get_key_success() {
        let entry = Entry::new(SERVICE, ACCOUNT).unwrap();

        let _ = entry.set_password(PASSWORD);

        let res = get_key(SERVICE, ACCOUNT);

        assert!(res.is_ok());

        assert_eq!(res.unwrap(), PASSWORD);

        let _ = entry.delete_credential();
    }

    #[test]

    fn test_delete_key_success() {
        let entry = Entry::new(SERVICE, ACCOUNT).unwrap();

        let _ = entry.set_password(PASSWORD);

        let res = delete_key(SERVICE, ACCOUNT);

        assert!(res.is_ok());

        let res_after_deletion = entry.get_password();

        assert!(res_after_deletion.is_err());
    }

    #[test]

    fn test_store_key_error() {
        let invalid_service = "";

        let res = store_key(invalid_service, ACCOUNT, PASSWORD);

        assert!(res.is_err());
    }

    #[test]

    fn test_get_key_error() {
        let invalid_service = "";

        let res = get_key(invalid_service, ACCOUNT);

        assert!(res.is_err());
    }

    #[test]

    fn test_delete_key_error() {
        let invalid_service = "";

        let res = delete_key(invalid_service, ACCOUNT);

        assert!(res.is_err());
    }
}
