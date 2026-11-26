use std::sync::Mutex;
use std::time::Duration;

const LOCAL_HOSTS: &str = "localhost,127.0.0.1,::1,*.localhost";
const PROXY_ENV_VARIABLES: [&str; 4] = ["ALL_PROXY", "all_proxy", "HTTPS_PROXY", "https_proxy"];

pub struct HttpClient {
    current: Mutex<(Option<String>, reqwest::Client)>,
}

impl HttpClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        let system_proxy = read_system_proxy();
        let client = build_client(system_proxy.as_deref())?;
        Ok(Self { current: Mutex::new((system_proxy, client)) })
    }

    pub fn current(&self) -> reqwest::Client {
        let system_proxy = read_system_proxy();
        let mut guard = match self.current.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.0 != system_proxy {
            if let Ok(client) = build_client(system_proxy.as_deref()) {
                *guard = (system_proxy, client);
            }
        }
        guard.1.clone()
    }
}

fn build_client(system_proxy: Option<&str>) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(180))
        .user_agent(concat!("Tarn/", env!("CARGO_PKG_VERSION")));
    let has_env_proxy = PROXY_ENV_VARIABLES.iter().any(|name| std::env::var_os(name).is_some());
    if !has_env_proxy {
        if let Some(address) = system_proxy.and_then(socks_address_from_system_proxy) {
            let proxy = reqwest::Proxy::all(format!("socks5h://{address}"))?
                .no_proxy(reqwest::NoProxy::from_string(LOCAL_HOSTS));
            builder = builder.proxy(proxy);
        }
    }
    builder.build()
}

fn socks_address_from_system_proxy(server: &str) -> Option<&str> {
    let entries: Vec<(&str, &str)> = server.split(';').filter_map(|entry| entry.trim().split_once('=')).collect();
    let has_https_proxy = entries.iter().any(|(scheme, _)| scheme.eq_ignore_ascii_case("https"));
    if has_https_proxy {
        return None;
    }
    entries
        .iter()
        .find(|(scheme, _)| scheme.eq_ignore_ascii_case("socks"))
        .map(|(_, address)| *address)
        .filter(|address| !address.is_empty())
}

#[cfg(windows)]
fn read_system_proxy() -> Option<String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enabled: u32 = settings.get_value("ProxyEnable").ok()?;
    if enabled == 0 {
        return None;
    }
    let server: String = settings.get_value("ProxyServer").ok()?;
    Some(server).filter(|value| !value.is_empty())
}

#[cfg(not(windows))]
fn read_system_proxy() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_socks_address_in_windows_proxy_setting() {
        assert_eq!(socks_address_from_system_proxy("socks=127.0.0.1:10808"), Some("127.0.0.1:10808"));
        assert_eq!(socks_address_from_system_proxy("http=a:1;socks=b:2"), Some("b:2"));
    }

    #[test]
    fn leaves_http_proxy_settings_to_reqwest() {
        assert_eq!(socks_address_from_system_proxy("127.0.0.1:8080"), None);
        assert_eq!(socks_address_from_system_proxy("http=a:1;https=a:1;socks=b:2"), None);
        assert_eq!(socks_address_from_system_proxy("socks="), None);
    }

    #[test]
    #[ignore = "нужен работающий системный SOCKS-прокси и выход в интернет"]
    fn live_system_proxy_reaches_anthropic() {
        tauri::async_runtime::block_on(async {
            let client = HttpClient::new().expect("клиент").current();
            let response = client.get("https://api.anthropic.com/v1/models").send().await.expect("ответ");
            println!("статус без ключа: {}", response.status());
            assert_eq!(response.status().as_u16(), 401);
        });
    }
}
