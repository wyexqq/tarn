use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::json;

use crate::claude_code_export::project_folder_name;
use crate::providers::ToolSpec;

const MAX_CHARS_PER_FILE: usize = 40_000;
const MAX_CHARS_TOTAL: usize = 120_000;
const CUT_MARK: &str = "\n[файл обрезан: он слишком большой]";

pub const TOOL_REMEMBER: &str = "remember";
pub const TOOL_FORGET: &str = "forget";
pub const TOOL_READ_MEMORY_NOTE: &str = "read_memory_note";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFile {
    pub source: String,
    pub title: String,
    pub path: String,
    pub text: String,
    pub was_cut: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnMemory {
    pub shared: String,
    pub project_text: Option<String>,
    pub project_folder: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MemoryPlaces {
    pub tarn_data: PathBuf,
    pub home: PathBuf,
    pub project: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
pub struct MemorySources {
    pub tarn: bool,
    pub claude: bool,
    pub codex: bool,
}

pub fn shared_memory_path(tarn_data: &Path) -> PathBuf {
    tarn_data.join("memory").join("shared.md")
}

pub fn project_memory_path(tarn_data: &Path, project: &Path) -> PathBuf {
    let file_name = format!("{}.md", project_folder_name(&project.to_string_lossy()));
    tarn_data.join("memory").join("projects").join(file_name)
}

fn folders_from_top_to_project(project: &Path, home: &Path) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for folder in project.ancestors() {
        if folder == home {
            break;
        }
        folders.push(folder.to_path_buf());
    }
    folders.reverse();
    folders
}

fn limited_text(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((index, _)) => (format!("{}{CUT_MARK}", &text[..index]), true),
        None => (text.to_string(), false),
    }
}

fn read_memory_file(source: &str, title: String, path: &Path) -> Option<MemoryFile> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    let (text, was_cut) = limited_text(&content, MAX_CHARS_PER_FILE);
    Some(MemoryFile { source: source.to_string(), title, path: path.to_string_lossy().to_string(), text, was_cut })
}

fn tarn_files(places: &MemoryPlaces) -> Vec<MemoryFile> {
    let mut files: Vec<MemoryFile> = read_memory_file("tarn", "Общая память".to_string(), &shared_memory_path(&places.tarn_data)).into_iter().collect();
    if let Some(project) = &places.project {
        files.extend(read_memory_file("tarn", "Память проекта".to_string(), &project_memory_path(&places.tarn_data, project)));
    }
    files
}

fn claude_memory_folder(home: &Path, folder: &Path) -> PathBuf {
    home.join(".claude").join("projects").join(project_folder_name(&folder.to_string_lossy())).join("memory")
}

fn claude_files(places: &MemoryPlaces) -> Vec<MemoryFile> {
    let mut files: Vec<MemoryFile> = read_memory_file("claude", "CLAUDE.md пользователя".to_string(), &places.home.join(".claude").join("CLAUDE.md")).into_iter().collect();
    let folders = places.project.as_deref().map_or(Vec::new(), |project| folders_from_top_to_project(project, &places.home));
    for folder in &folders {
        for relative in ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"] {
            files.extend(read_memory_file("claude", relative.to_string(), &folder.join(relative)));
        }
    }
    for folder in &folders {
        files.extend(read_memory_file("claude", "Память Claude Code".to_string(), &claude_memory_folder(&places.home, folder).join("MEMORY.md")));
    }
    files
}

fn codex_instructions_in(folder: &Path, title: &str) -> Option<MemoryFile> {
    read_memory_file("codex", title.to_string(), &folder.join("AGENTS.override.md"))
        .or_else(|| read_memory_file("codex", title.to_string(), &folder.join("AGENTS.md")))
}

fn codex_files(places: &MemoryPlaces) -> Vec<MemoryFile> {
    let mut files: Vec<MemoryFile> = codex_instructions_in(&places.home.join(".codex"), "AGENTS.md пользователя").into_iter().collect();
    let folders = places.project.as_deref().map_or(Vec::new(), |project| folders_from_top_to_project(project, &places.home));
    for folder in &folders {
        files.extend(codex_instructions_in(folder, "AGENTS.md"));
    }
    files
}

fn keep_within_total_limit(files: Vec<MemoryFile>) -> Vec<MemoryFile> {
    let mut kept = Vec::new();
    let mut remaining = MAX_CHARS_TOTAL;
    for file in files.into_iter().rev() {
        if remaining == 0 {
            break;
        }
        let (text, was_cut) = limited_text(&file.text, remaining);
        remaining = remaining.saturating_sub(file.text.chars().count());
        kept.push(MemoryFile { text, was_cut: file.was_cut || was_cut, ..file });
    }
    kept.reverse();
    kept
}

pub fn collect_memory(places: &MemoryPlaces, sources: MemorySources) -> Vec<MemoryFile> {
    let mut files = Vec::new();
    if sources.claude {
        files.extend(claude_files(places));
    }
    if sources.codex {
        files.extend(codex_files(places));
    }
    if sources.tarn {
        files.extend(tarn_files(places));
    }
    keep_within_total_limit(files)
}

pub fn system_section(files: &[MemoryFile]) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let bodies: Vec<String> = files
        .iter()
        .map(|file| format!("<memory title=\"{}\" path=\"{}\">\n{}\n</memory>", file.title, file.path, file.text))
        .collect();
    let introduction = "Ниже инструкции и память пользователя. Учитывай их в работе. Записи памяти могли устареть: если запись называет файл или настройку, проверь, что они ещё есть.";
    Some(format!("{introduction}\n\n{}", bodies.join("\n\n")))
}

pub fn tool_specs(sources: MemorySources, files: &[MemoryFile]) -> Vec<ToolSpec> {
    let where_property = json!({ "type": "string", "enum": ["shared", "project"], "description": "shared: для всех чатов, project: только для этой рабочей папки" });
    let mut specs = Vec::new();
    if sources.tarn {
        specs.push(ToolSpec {
            name: TOOL_REMEMBER.to_string(),
            description: "Запомнить факт надолго: предпочтение пользователя, решение, договорённость. Запись видна в следующих чатах. Секреты и ключи не записывать.".to_string(),
            schema: json!({ "type": "object", "properties": { "where": where_property, "text": { "type": "string" } }, "required": ["where", "text"] }),
        });
        specs.push(ToolSpec {
            name: TOOL_FORGET.to_string(),
            description: "Удалить из памяти строки, содержащие указанный фрагмент.".to_string(),
            schema: json!({ "type": "object", "properties": { "where": where_property, "fragment": { "type": "string" } }, "required": ["where", "fragment"] }),
        });
    }
    let has_claude_notes = files.iter().any(|file| file.source == "claude" && file.path.ends_with("MEMORY.md"));
    if has_claude_notes {
        specs.push(ToolSpec {
            name: TOOL_READ_MEMORY_NOTE.to_string(),
            description: "Прочитать заметку памяти Claude Code, на которую ссылается MEMORY.md, например feedback_style.md.".to_string(),
            schema: json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }),
        });
    }
    specs
}

fn memory_path_for(places: &MemoryPlaces, place: &str) -> Result<PathBuf, String> {
    match place {
        "shared" => Ok(shared_memory_path(&places.tarn_data)),
        "project" => places
            .project
            .as_deref()
            .map(|project| project_memory_path(&places.tarn_data, project))
            .ok_or_else(|| "У этого чата нет рабочей папки, запиши в общую память".to_string()),
        other => Err(format!("Нет такой памяти: {other}")),
    }
}

pub fn write_memory_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, text).map_err(|error| error.to_string())
}

pub fn remember(places: &MemoryPlaces, place: &str, text: &str) -> Result<String, String> {
    let line = text.trim();
    if line.is_empty() {
        return Err("Нечего запоминать: текст пустой".to_string());
    }
    let path = memory_path_for(places, place)?;
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line);
    content.push('\n');
    write_memory_text(&path, &content)?;
    Ok("Запомнил".to_string())
}

pub fn forget(places: &MemoryPlaces, place: &str, fragment: &str) -> Result<String, String> {
    let needle = fragment.trim();
    if needle.is_empty() {
        return Err("Не указан фрагмент".to_string());
    }
    let path = memory_path_for(places, place)?;
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let kept: Vec<&str> = content.lines().filter(|line| !line.contains(needle)).collect();
    let removed = content.lines().count() - kept.len();
    if removed == 0 {
        return Err("Таких строк в памяти нет".to_string());
    }
    let updated = if kept.is_empty() { String::new() } else { format!("{}\n", kept.join("\n")) };
    write_memory_text(&path, &updated)?;
    Ok(format!("Удалено строк: {removed}"))
}

fn is_plain_note_name(name: &str) -> bool {
    let has_no_path_parts = !name.contains(['/', '\\', ':']) && !name.contains("..");
    has_no_path_parts && name.to_lowercase().ends_with(".md")
}

pub fn read_claude_note(places: &MemoryPlaces, name: &str) -> Result<String, String> {
    if !is_plain_note_name(name) {
        return Err("Нужно только имя файла заметки, без папок".to_string());
    }
    let project = places.project.as_deref().ok_or_else(|| "У этого чата нет рабочей папки".to_string())?;
    let folders = folders_from_top_to_project(project, &places.home);
    let found = folders
        .iter()
        .rev()
        .map(|folder| claude_memory_folder(&places.home, folder).join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| format!("Заметка {name} не найдена"))?;
    let content = std::fs::read_to_string(found).map_err(|error| error.to_string())?;
    Ok(limited_text(&content, MAX_CHARS_PER_FILE).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_SOURCES: MemorySources = MemorySources { tarn: true, claude: true, codex: true };

    struct Sandbox {
        root: PathBuf,
        places: MemoryPlaces,
    }

    impl Sandbox {
        fn new() -> Sandbox {
            let root = std::env::temp_dir().join(format!("tarn-memory-{}", uuid::Uuid::new_v4()));
            let places = MemoryPlaces {
                tarn_data: root.join("tarn"),
                home: root.join("home"),
                project: Some(root.join("work").join("app")),
            };
            std::fs::create_dir_all(places.project.as_ref().unwrap()).unwrap();
            std::fs::create_dir_all(&places.home).unwrap();
            Sandbox { root, places }
        }

        fn project(&self) -> &Path {
            self.places.project.as_deref().unwrap()
        }

        fn write(&self, path: PathBuf, text: &str) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn texts(files: &[MemoryFile]) -> Vec<&str> {
        files.iter().map(|file| file.text.as_str()).collect()
    }

    #[test]
    fn reads_claude_codex_and_own_memory() {
        let sandbox = Sandbox::new();
        let home = sandbox.places.home.clone();
        sandbox.write(home.join(".claude").join("CLAUDE.md"), "клод общий");
        sandbox.write(sandbox.project().join("CLAUDE.md"), "клод проект");
        sandbox.write(sandbox.project().parent().unwrap().join("CLAUDE.md"), "клод папка выше");
        sandbox.write(claude_memory_folder(&home, sandbox.project()).join("MEMORY.md"), "клод память");
        sandbox.write(home.join(".codex").join("AGENTS.md"), "кодекс общий");
        sandbox.write(sandbox.project().join("AGENTS.md"), "кодекс проект");
        remember(&sandbox.places, "shared", "моя общая").unwrap();
        remember(&sandbox.places, "project", "моя проектная").unwrap();
        let files = collect_memory(&sandbox.places, ALL_SOURCES);
        assert_eq!(
            texts(&files),
            ["клод общий", "клод папка выше", "клод проект", "клод память", "кодекс общий", "кодекс проект", "моя общая\n", "моя проектная\n"]
        );
    }

    #[test]
    fn switched_off_sources_are_not_read() {
        let sandbox = Sandbox::new();
        sandbox.write(sandbox.project().join("CLAUDE.md"), "клод");
        sandbox.write(sandbox.project().join("AGENTS.md"), "кодекс");
        let only_codex = MemorySources { tarn: false, claude: false, codex: true };
        assert_eq!(texts(&collect_memory(&sandbox.places, only_codex)), ["кодекс"]);
    }

    #[test]
    fn folders_above_the_home_folder_are_not_read() {
        let sandbox = Sandbox::new();
        let home = sandbox.places.home.clone();
        let places = MemoryPlaces { project: Some(home.join("code")), ..sandbox.places.clone() };
        std::fs::create_dir_all(home.join("code")).unwrap();
        sandbox.write(home.join("CLAUDE.md"), "в домашней");
        sandbox.write(home.join("code").join("CLAUDE.md"), "в проекте");
        sandbox.write(sandbox.root.join("CLAUDE.md"), "выше домашней");
        assert_eq!(texts(&collect_memory(&places, ALL_SOURCES)), ["в проекте"]);
    }

    #[test]
    fn a_chat_without_a_folder_reads_only_general_files() {
        let sandbox = Sandbox::new();
        let places = MemoryPlaces { project: None, ..sandbox.places.clone() };
        sandbox.write(sandbox.project().join("CLAUDE.md"), "проект");
        sandbox.write(places.home.join(".claude").join("CLAUDE.md"), "общий");
        assert_eq!(texts(&collect_memory(&places, ALL_SOURCES)), ["общий"]);
        assert!(remember(&places, "project", "текст").is_err());
    }

    #[test]
    fn override_file_wins_over_agents_file() {
        let sandbox = Sandbox::new();
        sandbox.write(sandbox.project().join("AGENTS.md"), "обычный");
        sandbox.write(sandbox.project().join("AGENTS.override.md"), "важнее");
        let only_codex = MemorySources { tarn: false, claude: false, codex: true };
        assert_eq!(texts(&collect_memory(&sandbox.places, only_codex)), ["важнее"]);
    }

    #[test]
    fn large_files_are_cut_and_the_nearest_files_are_kept_first() {
        let sandbox = Sandbox::new();
        let huge = "я".repeat(MAX_CHARS_PER_FILE + 10);
        sandbox.write(sandbox.project().join("CLAUDE.md"), &huge);
        let files = collect_memory(&sandbox.places, ALL_SOURCES);
        assert!(files[0].was_cut);
        assert!(files[0].text.ends_with(CUT_MARK));

        let many: Vec<MemoryFile> = (0..5)
            .map(|index| MemoryFile { source: "claude".into(), title: index.to_string(), path: String::new(), text: "я".repeat(MAX_CHARS_PER_FILE), was_cut: false })
            .collect();
        let kept = keep_within_total_limit(many);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept.last().unwrap().title, "4");
    }

    #[test]
    fn forget_removes_only_matching_lines() {
        let sandbox = Sandbox::new();
        remember(&sandbox.places, "shared", "любит чай").unwrap();
        remember(&sandbox.places, "shared", "любит кофе").unwrap();
        assert_eq!(forget(&sandbox.places, "shared", "кофе").unwrap(), "Удалено строк: 1");
        let content = std::fs::read_to_string(shared_memory_path(&sandbox.places.tarn_data)).unwrap();
        assert_eq!(content, "любит чай\n");
        assert!(forget(&sandbox.places, "shared", "кофе").is_err());
        assert!(remember(&sandbox.places, "shared", "  ").is_err());
    }

    #[test]
    fn note_names_cannot_leave_the_memory_folder() {
        let sandbox = Sandbox::new();
        let home = sandbox.places.home.clone();
        sandbox.write(claude_memory_folder(&home, sandbox.project()).join("note.md"), "заметка");
        sandbox.write(home.join("secret.md"), "секрет");
        assert_eq!(read_claude_note(&sandbox.places, "note.md").unwrap(), "заметка");
        assert!(read_claude_note(&sandbox.places, "..\\..\\..\\..\\secret.md").is_err());
        assert!(read_claude_note(&sandbox.places, "../secret.md").is_err());
        assert!(read_claude_note(&sandbox.places, "secret.txt").is_err());
    }

    #[test]
    fn system_section_lists_files_and_is_empty_without_them() {
        assert!(system_section(&[]).is_none());
        let file = MemoryFile { source: "tarn".into(), title: "Общая память".into(), path: "p".into(), text: "факт".into(), was_cut: false };
        let section = system_section(&[file]).unwrap();
        assert!(section.contains("<memory title=\"Общая память\" path=\"p\">\nфакт\n</memory>"));
    }
}
