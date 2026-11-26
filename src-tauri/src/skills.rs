use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Database;

const SKILL_FILE: &str = "SKILL.md";
const PLUGIN_MANIFEST: &str = ".claude-plugin/plugin.json";
const PLUGIN_SERVERS_FILE: &str = ".mcp.json";
const PLUGIN_ROOT_VARIABLE: &str = "${CLAUDE_PLUGIN_ROOT}";
const PLUGIN_KINDS: [&str; 2] = ["plugins", "external_plugins"];
const MAX_LISTED_DESCRIPTION_CHARS: usize = 240;

const DISABLED_SKILLS_SETTING: &str = "disabledSkills";
const SKILL_FOLDERS_SETTING: &str = "skillFolders";
const ENABLED_PLUGINS_SETTING: &str = "enabledPlugins";
const MANUAL_SERVERS_SETTING: &str = "mcpServers";
const DISABLED_SERVERS_SETTING: &str = "disabledMcpServers";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub folder: String,
    pub enabled: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub skill_count: usize,
    pub server_names: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct McpServerConfig {
    pub name: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: Option<String>,
    pub source: String,
}

fn list_setting(db: &Database, key: &str) -> Vec<String> {
    db.get_setting(key)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_list_setting(db: &Database, key: &str, items: &[String]) -> Result<(), String> {
    let text = serde_json::to_string(items).map_err(|error| error.to_string())?;
    db.set_setting(key, &text)
}

fn set_list_membership(db: &Database, key: &str, item: &str, is_member: bool) -> Result<(), String> {
    let mut items = list_setting(db, key);
    items.retain(|existing| existing != item);
    if is_member {
        items.push(item.to_string());
    }
    save_list_setting(db, key, &items)
}

fn unquote(value: &str) -> &str {
    let trimmed = value.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| trimmed.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')))
        .unwrap_or(trimmed)
}

fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    let mut collected: Option<String> = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            let value = value.trim();
            if value == ">" || value == "|" || value.is_empty() {
                collected = Some(String::new());
            } else {
                return Some(unquote(value).to_string());
            }
        } else if let Some(existing) = collected.as_mut() {
            if !line.starts_with(' ') {
                break;
            }
            if !existing.is_empty() {
                existing.push(' ');
            }
            existing.push_str(line.trim());
        }
    }
    collected.filter(|value| !value.is_empty())
}

fn folders_inside(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    folders
}

fn skill_from_folder(folder: &Path, source: &str, disabled: &[String]) -> Option<Skill> {
    let text = std::fs::read_to_string(folder.join(SKILL_FILE)).ok()?;
    let folder_name = folder.file_name()?.to_string_lossy().to_string();
    let name = frontmatter_value(&text, "name").unwrap_or(folder_name);
    let id = format!("{source}:{name}");
    Some(Skill {
        enabled: !disabled.contains(&id),
        id,
        name,
        description: frontmatter_value(&text, "description").unwrap_or_default(),
        source: source.to_string(),
        folder: folder.to_string_lossy().to_string(),
    })
}

fn skills_inside(root: &Path, source: &str, disabled: &[String]) -> Vec<Skill> {
    folders_inside(root)
        .iter()
        .filter_map(|folder| skill_from_folder(folder, source, disabled))
        .collect()
}

fn claude_home() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude"))
}

fn codex_home() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex"))
}

fn catalog_folders(claude_root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for marketplace in folders_inside(&claude_root.join("plugins").join("marketplaces")) {
        let marketplace_name = marketplace.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
        for kind in PLUGIN_KINDS {
            for folder in folders_inside(&marketplace.join(kind)) {
                let plugin_name = folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
                found.push((format!("{marketplace_name}/{plugin_name}"), folder));
            }
        }
    }
    found
}

fn plugin_name_of(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

fn expand_plugin_root(text: &str, folder: &Path) -> String {
    text.replace(PLUGIN_ROOT_VARIABLE, &folder.to_string_lossy())
}

fn server_from_json(name: &str, entry: &Value, folder: &Path, source: &str) -> McpServerConfig {
    let args = entry["args"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).map(|arg| expand_plugin_root(arg, folder)).collect())
        .unwrap_or_default();
    let env = entry["env"]
        .as_object()
        .map(|items| {
            items
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), expand_plugin_root(text, folder))))
                .collect()
        })
        .unwrap_or_default();
    McpServerConfig {
        name: name.to_string(),
        command: entry["command"].as_str().map(|command| expand_plugin_root(command, folder)),
        args,
        env,
        url: entry["url"].as_str().map(String::from),
        source: source.to_string(),
    }
}

fn servers_of_plugin(folder: &Path, source: &str) -> Vec<McpServerConfig> {
    let Ok(text) = std::fs::read_to_string(folder.join(PLUGIN_SERVERS_FILE)) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let section = if parsed["mcpServers"].is_object() { &parsed["mcpServers"] } else { &parsed };
    section
        .as_object()
        .map(|servers| {
            servers
                .iter()
                .filter(|(_, entry)| entry.is_object())
                .map(|(name, entry)| server_from_json(name, entry, folder, source))
                .collect()
        })
        .unwrap_or_default()
}

fn plugin_from_folder(id: &str, folder: &Path, enabled: bool) -> Plugin {
    let manifest: Value = std::fs::read_to_string(folder.join(PLUGIN_MANIFEST))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    Plugin {
        id: id.to_string(),
        name: manifest["name"].as_str().unwrap_or(plugin_name_of(id)).to_string(),
        description: manifest["description"].as_str().unwrap_or_default().to_string(),
        enabled,
        skill_count: skills_inside(&folder.join("skills"), "", &[]).len(),
        server_names: servers_of_plugin(folder, "").into_iter().map(|server| server.name).collect(),
    }
}

fn enabled_plugin_folders(db: &Database, claude_root: &Path) -> Vec<(String, PathBuf)> {
    let enabled = list_setting(db, ENABLED_PLUGINS_SETTING);
    if enabled.is_empty() {
        return Vec::new();
    }
    catalog_folders(claude_root)
        .into_iter()
        .filter(|(id, _)| enabled.contains(id))
        .collect()
}

pub fn list_plugins(db: &Database) -> Vec<Plugin> {
    let Some(claude_root) = claude_home() else {
        return Vec::new();
    };
    let enabled = list_setting(db, ENABLED_PLUGINS_SETTING);
    let mut plugins: Vec<Plugin> = catalog_folders(&claude_root)
        .iter()
        .map(|(id, folder)| plugin_from_folder(id, folder, enabled.contains(id)))
        .collect();
    plugins.sort_by(|left, right| right.enabled.cmp(&left.enabled).then_with(|| left.name.cmp(&right.name)));
    plugins
}

pub fn set_plugin_enabled(db: &Database, id: &str, enabled: bool) -> Result<(), String> {
    set_list_membership(db, ENABLED_PLUGINS_SETTING, id, enabled)
}

pub fn list_skills(db: &Database) -> Vec<Skill> {
    let disabled = list_setting(db, DISABLED_SKILLS_SETTING);
    let mut skills = Vec::new();
    if let Some(root) = claude_home() {
        skills.extend(skills_inside(&root.join("skills"), "Claude Code", &disabled));
        for (id, folder) in enabled_plugin_folders(db, &root) {
            let source = format!("Плагин {}", plugin_name_of(&id));
            skills.extend(skills_inside(&folder.join("skills"), &source, &disabled));
        }
    }
    if let Some(root) = codex_home() {
        skills.extend(skills_inside(&root.join("skills"), "Codex", &disabled));
    }
    for folder in list_setting(db, SKILL_FOLDERS_SETTING) {
        skills.extend(skills_inside(Path::new(&folder), "Своя папка", &disabled));
    }
    skills
}

pub fn enabled_skills(db: &Database) -> Vec<Skill> {
    list_skills(db).into_iter().filter(|skill| skill.enabled).collect()
}

pub fn set_skill_enabled(db: &Database, id: &str, enabled: bool) -> Result<(), String> {
    set_list_membership(db, DISABLED_SKILLS_SETTING, id, !enabled)
}

pub fn add_skill_folder(db: &Database, folder: &str) -> Result<usize, String> {
    let found = skills_inside(Path::new(folder), "Своя папка", &[]).len();
    if found == 0 {
        return Err("В этой папке нет скиллов: ищу подпапки с файлом SKILL.md".to_string());
    }
    set_list_membership(db, SKILL_FOLDERS_SETTING, folder, true)?;
    Ok(found)
}

pub fn read_skill(skill: &Skill) -> Result<String, String> {
    let body = std::fs::read_to_string(Path::new(&skill.folder).join(SKILL_FILE)).map_err(|_| format!("Не удалось прочитать скилл {}", skill.name))?;
    Ok(format!("Папка скилла: {}\n\n{body}", skill.folder))
}

pub fn invoked_skill_name(user_text: &str) -> Option<&str> {
    let first_word = user_text.trim_start().split_whitespace().next()?;
    first_word.strip_prefix('/').filter(|name| !name.is_empty())
}

pub fn describe_for_system_prompt(skills: &[Skill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut text = String::from("Доступные скиллы. Когда скилл подходит к задаче, сначала прочитай его инструментом read_skill:");
    for skill in skills {
        let description: String = skill.description.chars().take(MAX_LISTED_DESCRIPTION_CHARS).collect();
        text.push_str(&format!("\n- {}: {description}", skill.name));
    }
    Some(text)
}

pub fn manual_servers(db: &Database) -> Vec<McpServerConfig> {
    db.get_setting(MANUAL_SERVERS_SETTING)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_manual_servers(db: &Database, servers: &[McpServerConfig]) -> Result<(), String> {
    let text = serde_json::to_string(servers).map_err(|error| error.to_string())?;
    db.set_setting(MANUAL_SERVERS_SETTING, &text)
}

pub fn split_command_line(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for symbol in line.chars() {
        match symbol {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(symbol),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn add_manual_server(db: &Database, name: &str, command_or_url: &str) -> Result<(), String> {
    let name = name.trim();
    let target = command_or_url.trim();
    if name.is_empty() || target.is_empty() {
        return Err("Укажи название и команду или адрес".to_string());
    }
    if all_servers(db).iter().any(|server| server.name == name) {
        return Err(format!("Сервер {name} уже есть"));
    }
    let mut server = McpServerConfig { name: name.to_string(), source: "Вручную".to_string(), ..Default::default() };
    if target.starts_with("http://") || target.starts_with("https://") {
        server.url = Some(target.to_string());
    } else {
        let mut words = split_command_line(target).into_iter();
        server.command = words.next();
        server.args = words.collect();
    }
    let mut servers = manual_servers(db);
    servers.push(server);
    save_manual_servers(db, &servers)
}

pub fn remove_manual_server(db: &Database, name: &str) -> Result<(), String> {
    let mut servers = manual_servers(db);
    servers.retain(|server| server.name != name);
    save_manual_servers(db, &servers)
}

pub fn all_servers(db: &Database) -> Vec<McpServerConfig> {
    let mut servers = manual_servers(db);
    if let Some(root) = claude_home() {
        for (id, folder) in enabled_plugin_folders(db, &root) {
            let source = format!("Плагин {}", plugin_name_of(&id));
            for server in servers_of_plugin(&folder, &source) {
                if !servers.iter().any(|existing| existing.name == server.name) {
                    servers.push(server);
                }
            }
        }
    }
    servers
}

pub fn server_is_enabled(db: &Database, name: &str) -> bool {
    !list_setting(db, DISABLED_SERVERS_SETTING).iter().any(|disabled| disabled == name)
}

pub fn set_server_enabled(db: &Database, name: &str, enabled: bool) -> Result<(), String> {
    set_list_membership(db, DISABLED_SERVERS_SETTING, name, !enabled)
}

pub fn enabled_servers(db: &Database) -> Vec<McpServerConfig> {
    all_servers(db).into_iter().filter(|server| server_is_enabled(db, &server.name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_folder() -> PathBuf {
        let folder = std::env::temp_dir().join(format!("tarn-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn reads_name_and_description_from_frontmatter() {
        let text = "---\nname: code-review\ndescription: \"Проверка кода\"\n---\nТекст";
        assert_eq!(frontmatter_value(text, "name").as_deref(), Some("code-review"));
        assert_eq!(frontmatter_value(text, "description").as_deref(), Some("Проверка кода"));
        let folded = "---\nname: a\ndescription: >\n  первая часть\n  вторая часть\nlicense: MIT\n---\n";
        assert_eq!(frontmatter_value(folded, "description").as_deref(), Some("первая часть вторая часть"));
        assert_eq!(frontmatter_value("без шапки", "name"), None);
    }

    #[test]
    fn finds_skills_in_subfolders_and_respects_disabled_list() {
        let root = temp_folder();
        std::fs::create_dir_all(root.join("pdf")).unwrap();
        std::fs::write(root.join("pdf").join(SKILL_FILE), "---\ndescription: PDF\n---\n").unwrap();
        std::fs::create_dir_all(root.join("пустая")).unwrap();
        let skills = skills_inside(&root, "Codex", &["Codex:pdf".to_string()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "pdf");
        assert!(!skills[0].enabled);
    }

    #[test]
    fn plugin_servers_get_the_plugin_folder_in_their_paths() {
        let folder = temp_folder();
        std::fs::write(
            folder.join(PLUGIN_SERVERS_FILE),
            r#"{"mcpServers":{"a":{"command":"bun","args":["run","--cwd","${CLAUDE_PLUGIN_ROOT}"]},"b":{"type":"http","url":"https://x/mcp"}}}"#,
        )
        .unwrap();
        let servers = servers_of_plugin(&folder, "Плагин");
        let local = servers.iter().find(|server| server.name == "a").unwrap();
        assert_eq!(local.args[2], folder.to_string_lossy());
        let remote = servers.iter().find(|server| server.name == "b").unwrap();
        assert_eq!(remote.url.as_deref(), Some("https://x/mcp"));
    }

    #[test]
    fn splits_command_line_with_quoted_paths() {
        assert_eq!(split_command_line(r#"node "C:\Program Files\a.js" --x"#), vec!["node", r"C:\Program Files\a.js", "--x"]);
    }

    #[test]
    fn recognizes_skill_invocation_at_message_start() {
        assert_eq!(invoked_skill_name("/pdf собери файл"), Some("pdf"));
        assert_eq!(invoked_skill_name("открой /pdf"), None);
        assert_eq!(invoked_skill_name("/"), None);
    }
}
