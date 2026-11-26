use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::tools::IGNORED_FOLDERS;

const MAX_SHOWN_FILE_BYTES: u64 = 200 * 1024;
const MAX_LISTED_ENTRIES: usize = 400;
const MAX_DIFF_CELLS: usize = 4_000_000;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderEntry {
    pub name: String,
    pub relative_path: String,
    pub is_folder: bool,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: &'static str,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub relative_path: String,
    pub is_new: bool,
    pub is_deleted: bool,
    pub added: usize,
    pub removed: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub text: Option<String>,
    pub diff: Option<Vec<DiffLine>>,
    pub problem: Option<String>,
}

pub struct ChatFileChange {
    pub path: String,
    pub existed_before: bool,
    pub content_before: Option<Vec<u8>>,
}

pub fn inside_folder(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let given = Path::new(relative);
    let stays_inside = given.components().all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if !stays_inside {
        return Err("Путь выходит за пределы папки чата".to_string());
    }
    Ok(root.join(given))
}

pub fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

pub fn comparable_path(path: &Path) -> PathBuf {
    plain_path(&path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn relative_text(root: &Path, path: &Path) -> String {
    let root = comparable_path(root);
    let path = comparable_path(path);
    path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace('\\', "/")
}

pub fn list_folder(root: &Path, relative: &str) -> Result<Vec<FolderEntry>, String> {
    let folder = inside_folder(root, relative)?;
    let reader = std::fs::read_dir(&folder).map_err(|error| format!("Не удалось открыть папку: {error}"))?;
    let mut entries = Vec::new();
    for entry in reader.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && IGNORED_FOLDERS.contains(&name.as_str()) {
            continue;
        }
        let size = if kind.is_dir() { 0 } else { entry.metadata().map(|metadata| metadata.len()).unwrap_or(0) };
        entries.push(FolderEntry { relative_path: relative_text(root, &entry.path()), name, is_folder: kind.is_dir(), size });
    }
    entries.sort_by(|left, right| right.is_folder.cmp(&left.is_folder).then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase())));
    entries.truncate(MAX_LISTED_ENTRIES);
    Ok(entries)
}

fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines().collect()
}

fn whole_replacement(before: &[&str], after: &[&str]) -> Vec<DiffLine> {
    let removed = before.iter().map(|line| DiffLine { kind: "removed", text: line.to_string() });
    let added = after.iter().map(|line| DiffLine { kind: "added", text: line.to_string() });
    removed.chain(added).collect()
}

fn middle_diff(before: &[&str], after: &[&str]) -> Vec<DiffLine> {
    if before.len().saturating_mul(after.len()) > MAX_DIFF_CELLS {
        return whole_replacement(before, after);
    }
    let width = after.len() + 1;
    let mut common = vec![0u32; (before.len() + 1) * width];
    for row in (0..before.len()).rev() {
        for column in (0..after.len()).rev() {
            common[row * width + column] = if before[row] == after[column] {
                common[(row + 1) * width + column + 1] + 1
            } else {
                common[(row + 1) * width + column].max(common[row * width + column + 1])
            };
        }
    }
    let mut result = Vec::new();
    let (mut row, mut column) = (0, 0);
    while row < before.len() && column < after.len() {
        if before[row] == after[column] {
            result.push(DiffLine { kind: "same", text: before[row].to_string() });
            row += 1;
            column += 1;
        } else if common[(row + 1) * width + column] >= common[row * width + column + 1] {
            result.push(DiffLine { kind: "removed", text: before[row].to_string() });
            row += 1;
        } else {
            result.push(DiffLine { kind: "added", text: after[column].to_string() });
            column += 1;
        }
    }
    result.extend(before[row..].iter().map(|line| DiffLine { kind: "removed", text: line.to_string() }));
    result.extend(after[column..].iter().map(|line| DiffLine { kind: "added", text: line.to_string() }));
    result
}

pub fn line_diff(before: &str, after: &str) -> Vec<DiffLine> {
    let before_lines = lines_of(before);
    let after_lines = lines_of(after);
    let same_start = before_lines.iter().zip(&after_lines).take_while(|(left, right)| left == right).count();
    let before_rest = &before_lines[same_start..];
    let after_rest = &after_lines[same_start..];
    let same_end = before_rest.iter().rev().zip(after_rest.iter().rev()).take_while(|(left, right)| left == right).count();
    let mut result: Vec<DiffLine> = before_lines[..same_start].iter().map(|line| DiffLine { kind: "same", text: line.to_string() }).collect();
    result.extend(middle_diff(&before_rest[..before_rest.len() - same_end], &after_rest[..after_rest.len() - same_end]));
    result.extend(before_rest[before_rest.len() - same_end..].iter().map(|line| DiffLine { kind: "same", text: line.to_string() }));
    result
}

fn count_kind(diff: &[DiffLine], kind: &str) -> usize {
    diff.iter().filter(|line| line.kind == kind).count()
}

fn read_shown_text(path: &Path) -> Result<Option<String>, String> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(None);
    };
    if metadata.len() > MAX_SHOWN_FILE_BYTES {
        return Err("Файл слишком большой для просмотра".to_string());
    }
    let bytes = std::fs::read(path).map_err(|error| format!("Не удалось прочитать файл: {error}"))?;
    String::from_utf8(bytes).map(Some).map_err(|_| "Это не текстовый файл".to_string())
}

fn text_before(change: &ChatFileChange) -> Option<String> {
    if !change.existed_before {
        return Some(String::new());
    }
    let bytes = change.content_before.as_ref()?;
    String::from_utf8(bytes.clone()).ok()
}

pub fn first_changes_by_path(changes: Vec<ChatFileChange>) -> Vec<ChatFileChange> {
    let mut seen = std::collections::HashSet::new();
    changes.into_iter().filter(|change| seen.insert(change.path.clone())).collect()
}

pub fn summarize_changes(root: Option<&Path>, changes: Vec<ChatFileChange>) -> Vec<ChangedFile> {
    first_changes_by_path(changes)
        .into_iter()
        .map(|change| {
            let path = Path::new(&change.path);
            let now = read_shown_text(path).ok().flatten();
            let diff = match (text_before(&change), &now) {
                (Some(before), Some(after)) => line_diff(&before, after),
                (Some(before), None) => line_diff(&before, ""),
                _ => Vec::new(),
            };
            ChangedFile {
                relative_path: root.map_or_else(|| change.path.clone(), |root| relative_text(root, path)),
                is_new: !change.existed_before,
                is_deleted: !path.exists(),
                added: count_kind(&diff, "added"),
                removed: count_kind(&diff, "removed"),
                path: change.path,
            }
        })
        .collect()
}

pub fn view_file(path: &Path, change: Option<ChatFileChange>) -> FileView {
    let shown = path.display().to_string();
    let now = match read_shown_text(path) {
        Ok(text) => text,
        Err(problem) => return FileView { path: shown, text: None, diff: None, problem: Some(problem) },
    };
    let diff = change.as_ref().and_then(text_before).map(|before| line_diff(&before, now.as_deref().unwrap_or_default()));
    FileView { path: shown, text: now, diff, problem: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(diff: &[DiffLine]) -> String {
        diff.iter().map(|line| match line.kind { "same" => '=', "added" => '+', _ => '-' }).collect()
    }

    #[test]
    fn diff_keeps_common_lines_and_marks_changes() {
        let diff = line_diff("а\nб\nв\nг", "а\nб2\nв\nг\nд");
        assert_eq!(kinds(&diff), "=-+==+");
        assert_eq!(diff[1].text, "б");
        assert_eq!(diff[2].text, "б2");
        assert_eq!(kinds(&line_diff("", "x\ny")), "++");
        assert_eq!(kinds(&line_diff("x", "")), "-");
    }

    #[test]
    fn paths_cannot_leave_the_chat_folder() {
        let root = Path::new("C:\\проект");
        assert!(inside_folder(root, "src/main.rs").is_ok());
        assert!(inside_folder(root, "..\\чужое").is_err());
        assert!(inside_folder(root, "C:\\Windows").is_err());
    }

    #[test]
    fn folder_listing_puts_folders_first_and_skips_service_ones() {
        let root = std::env::temp_dir().join(format!("tarn-files-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("Б.txt"), "1").unwrap();
        std::fs::write(root.join("а.txt"), "22").unwrap();
        let listed = list_folder(&root, "").unwrap();
        let names: Vec<&str> = listed.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["src", "а.txt", "Б.txt"]);
        assert_eq!(listed[1].size, 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn changes_are_counted_against_the_first_version() {
        let root = std::env::temp_dir().join(format!("tarn-changes-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "один\nдва\nтри").unwrap();
        let path = file.display().to_string();
        let changes = vec![
            ChatFileChange { path: path.clone(), existed_before: true, content_before: Some("один\nтри".as_bytes().to_vec()) },
            ChatFileChange { path: path.clone(), existed_before: true, content_before: Some("один\nдва".as_bytes().to_vec()) },
        ];
        let summary = summarize_changes(Some(&root), changes);
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].relative_path, "a.txt");
        assert_eq!((summary[0].added, summary[0].removed), (1, 0));
        let _ = std::fs::remove_dir_all(root);
    }
}
