use crate::models::FmError;
use keyring_core::Entry;
use std::collections::HashMap;

const SERVICE: &str = "com.furman.s3";

/// Register the platform-native credential store with keyring-core.
/// Call once at process startup, before any keychain_* command runs.
pub fn init() -> Result<(), FmError> {
    let config: HashMap<&str, &str> = HashMap::new();
    #[cfg(target_os = "macos")]
    {
        let store = apple_native_keyring_store::keychain::Store::new_with_configuration(&config)
            .map_err(|e| FmError::Other(format!("keyring store init: {e}")))?;
        keyring_core::set_default_store(store);
    }
    #[cfg(target_os = "linux")]
    {
        let store = dbus_secret_service_keyring_store::Store::new_with_configuration(&config)
            .map_err(|e| FmError::Other(format!("keyring store init: {e}")))?;
        keyring_core::set_default_store(store);
    }
    let _ = config; // suppress unused warning on other platforms
    Ok(())
}

fn entry_for(profile_id: &str) -> Result<Entry, FmError> {
    Entry::new(SERVICE, profile_id).map_err(|e| FmError::Other(format!("keyring: {e}")))
}

#[tauri::command]
pub fn keychain_set(profile_id: String, secret: String) -> Result<(), FmError> {
    let entry = entry_for(&profile_id)?;
    entry
        .set_password(&secret)
        .map_err(|e| FmError::Other(format!("keychain set: {e}")))
}

#[tauri::command]
pub fn keychain_get(profile_id: String) -> Result<Option<String>, FmError> {
    let entry = entry_for(&profile_id)?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(FmError::Other(format!("keychain get: {e}"))),
    }
}

#[tauri::command]
pub fn keychain_delete(profile_id: String) -> Result<(), FmError> {
    let entry = entry_for(&profile_id)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(e) => Err(FmError::Other(format!("keychain delete: {e}"))),
    }
}
