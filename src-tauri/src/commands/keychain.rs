use crate::models::FmError;
use keyring::Entry;

const SERVICE: &str = "com.furman.s3";

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
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(FmError::Other(format!("keychain get: {e}"))),
    }
}

#[tauri::command]
pub fn keychain_delete(profile_id: String) -> Result<(), FmError> {
    let entry = entry_for(&profile_id)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(FmError::Other(format!("keychain delete: {e}"))),
    }
}
