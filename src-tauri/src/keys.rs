const DEFAULT_SERVICE: &str = "tarn";

fn service_name() -> String {
    std::env::var("TARN_KEYRING_SERVICE").unwrap_or_else(|_| DEFAULT_SERVICE.to_string())
}

fn entry_for(provider_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(&service_name(), provider_id)
        .map_err(|error| format!("хранилище ключей недоступно: {error}"))
}

pub fn save(provider_id: &str, key: &str) -> Result<(), String> {
    entry_for(provider_id)?
        .set_password(key)
        .map_err(|error| format!("не удалось сохранить ключ: {error}"))
}

pub fn load(provider_id: &str) -> Option<String> {
    entry_for(provider_id).ok()?.get_password().ok()
}

pub fn exists(provider_id: &str) -> bool {
    load(provider_id).is_some()
}

pub fn remove(provider_id: &str) -> Result<(), String> {
    match entry_for(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("не удалось удалить ключ: {error}")),
    }
}
