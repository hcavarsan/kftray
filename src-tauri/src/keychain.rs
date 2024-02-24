use keyring::{Entry, Error as KeyringError};
use log::{error, info};
use tauri::{Error as TauriError, InvokeError};

// Define a custom error type that encapsulates errors from different sources.
#[derive(Debug)]
pub enum CustomError {
    Keyring(KeyringError),
    Tauri(TauriError),
}

// Implement conversion from `KeyringError` to `CustomError`.
impl From<KeyringError> for CustomError {
    fn from(error: KeyringError) -> Self {
        error!("Keyring error occurred: {:?}", error);
        CustomError::Keyring(error)
    }
}

// Implement conversion from `TauriError` to `CustomError`.
impl From<TauriError> for CustomError {
    fn from(error: TauriError) -> Self {
        error!("Tauri error occurred: {:?}", error);
        CustomError::Tauri(error)
    }
}

/// Implements the conversion from `CustomError` to `InvokeError`.
impl From<CustomError> for InvokeError {
    fn from(error: CustomError) -> Self {
        match error {
            CustomError::Keyring(err) => {
                error!("Converting to InvokeError: {:?}", err);
                InvokeError::from(err)
            }
            CustomError::Tauri(err) => {
                error!("Converting to InvokeError: {:?}", err);
                InvokeError::from(err)
            }
        }
    }
}

/// Stores a key using the `keyring` crate.
pub fn store_key(service: &str, name: &str, password: &str) -> Result<(), CustomError> {
    let entry = Entry::new(service, name)?;
    entry.set_password(password)?;
    info!(
        "Stored password for service '{}' and name '{}'",
        service, name
    );
    Ok(())
}

/// Retrieves a key using the `keyring` crate.
#[tauri::command]
pub fn get_key(service: &str, name: &str) -> Result<String, CustomError> {
    let entry = Entry::new(service, name)?;
    let password = entry.get_password()?;
    info!(
        "Retrieved password for service '{}' and name '{}'",
        service, name
    );
    Ok(password)
}

/// Deletes a key using the `keyring` crate.
#[tauri::command]
pub fn delete_key(service: &str, name: &str) -> Result<(), CustomError> {
    let entry = Entry::new(service, name)?;
    entry.delete_password()?;
    info!(
        "Deleted password for service '{}' and name '{}'",
        service, name
    );
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
        println!("Starting test_store_key_success");
        let res = store_key(SERVICE, ACCOUNT, PASSWORD);
        assert!(res.is_ok());
        println!("store_key succeeded");

        let entry = Entry::new(SERVICE, ACCOUNT).unwrap();
        let delete_result = entry.delete_password();
        println!("Tried to delete password: {:?}", delete_result);
        assert!(delete_result.is_ok());
        println!("Password deleted successfully");
    }
    #[test]
    fn test_get_key_success() {
        let entry = Entry::new(SERVICE, ACCOUNT).unwrap();
        let _ = entry.set_password(PASSWORD);

        let res = get_key(SERVICE, ACCOUNT);
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), PASSWORD);

        let _ = entry.delete_password();
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
