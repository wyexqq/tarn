use std::io::ErrorKind;
use std::path::Path;

use sha2::{Digest, Sha256};

const MAX_SAVED_FILE_BYTES: u64 = 20 * 1024 * 1024;

pub struct FileBefore {
    existed: bool,
    content: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub existed_before: bool,
    pub content_before: Option<Vec<u8>>,
    pub hash_after: Option<String>,
}

impl FileChange {
    fn earlier_content_was_not_saved(&self) -> bool {
        self.existed_before && self.content_before.is_none()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UndoEntry {
    pub path: String,
    pub content: Option<Vec<u8>>,
    pub hash_after_restore: Option<String>,
}

#[derive(Debug, Default)]
pub struct RestoreReport {
    pub undo: Vec<UndoEntry>,
    pub not_restored: Vec<String>,
}

fn hash_of(content: Option<&[u8]>) -> Option<String> {
    content.map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}

fn read_if_exists(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn put_content(path: &Path, content: Option<&[u8]>) -> std::io::Result<()> {
    let Some(bytes) = content else {
        return std::fs::remove_file(path);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
}

fn file_name_of(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_string(), |name| name.to_string_lossy().to_string())
}

pub fn read_before_change(path: &Path) -> FileBefore {
    match std::fs::metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => FileBefore { existed: false, content: None },
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_SAVED_FILE_BYTES => {
            FileBefore { existed: true, content: std::fs::read(path).ok() }
        }
        _ => FileBefore { existed: true, content: None },
    }
}

pub fn change_after_write(path: &Path, before: FileBefore) -> FileChange {
    let content_now = read_if_exists(path).ok().flatten();
    FileChange {
        path: path.to_string_lossy().to_string(),
        existed_before: before.existed,
        content_before: before.content,
        hash_after: hash_of(content_now.as_deref()),
    }
}

fn restore_one(change: &FileChange) -> Option<UndoEntry> {
    if change.earlier_content_was_not_saved() {
        return None;
    }
    let path = Path::new(&change.path);
    let content_now = read_if_exists(path).ok()?;
    let changed_since_the_model = hash_of(content_now.as_deref()) != change.hash_after;
    if changed_since_the_model {
        return None;
    }
    put_content(path, change.content_before.as_deref()).ok()?;
    Some(UndoEntry {
        path: change.path.clone(),
        content: content_now,
        hash_after_restore: hash_of(change.content_before.as_deref()),
    })
}

pub fn restore(changes: &[FileChange]) -> RestoreReport {
    let mut report = RestoreReport::default();
    for change in changes {
        match restore_one(change) {
            Some(entry) => report.undo.push(entry),
            None => report.not_restored.push(file_name_of(&change.path)),
        }
    }
    report
}

fn undo_one(entry: &UndoEntry) -> Option<()> {
    let path = Path::new(&entry.path);
    let content_now = read_if_exists(path).ok()?;
    let changed_since_the_restore = hash_of(content_now.as_deref()) != entry.hash_after_restore;
    if changed_since_the_restore {
        return None;
    }
    put_content(path, entry.content.as_deref()).ok()
}

pub fn undo(entries: &[UndoEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| undo_one(entry).is_none())
        .map(|entry| file_name_of(&entry.path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_file(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("tarn-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        folder.join(name)
    }

    fn model_writes(path: &Path, text: &str) -> FileChange {
        let before = read_before_change(path);
        std::fs::write(path, text).unwrap();
        change_after_write(path, before)
    }

    #[test]
    fn restore_brings_back_the_earlier_text_and_undo_brings_back_the_model_text() {
        let path = temp_file("a.txt");
        std::fs::write(&path, "было").unwrap();
        let change = model_writes(&path, "стало");
        let report = restore(&[change]);
        assert!(report.not_restored.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "было");
        assert!(undo(&report.undo).is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "стало");
    }

    #[test]
    fn restore_deletes_a_file_the_model_created_and_undo_creates_it_again() {
        let path = temp_file("new.txt");
        let change = model_writes(&path, "новый");
        let report = restore(&[change]);
        assert!(!path.exists());
        assert!(undo(&report.undo).is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "новый");
    }

    #[test]
    fn restore_keeps_binary_content_exact() {
        let path = temp_file("b.bin");
        let earlier = vec![0u8, 255, 1, 254, 0];
        std::fs::write(&path, &earlier).unwrap();
        let change = model_writes(&path, "текст");
        restore(&[change]);
        assert_eq!(std::fs::read(&path).unwrap(), earlier);
    }

    #[test]
    fn restore_leaves_a_file_changed_after_the_model_alone() {
        let path = temp_file("mine.txt");
        std::fs::write(&path, "было").unwrap();
        let change = model_writes(&path, "стало");
        std::fs::write(&path, "моя правка").unwrap();
        let report = restore(&[change]);
        assert_eq!(report.not_restored, vec!["mine.txt"]);
        assert!(report.undo.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "моя правка");
    }

    #[test]
    fn restore_does_not_recreate_a_file_deleted_after_the_model() {
        let path = temp_file("gone.txt");
        std::fs::write(&path, "было").unwrap();
        let change = model_writes(&path, "стало");
        std::fs::remove_file(&path).unwrap();
        let report = restore(&[change]);
        assert_eq!(report.not_restored, vec!["gone.txt"]);
        assert!(!path.exists());
    }

    #[test]
    fn undo_leaves_a_file_changed_after_the_restore_alone() {
        let path = temp_file("later.txt");
        std::fs::write(&path, "было").unwrap();
        let change = model_writes(&path, "стало");
        let report = restore(&[change]);
        std::fs::write(&path, "новая правка").unwrap();
        assert_eq!(undo(&report.undo), vec!["later.txt"]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "новая правка");
    }

    #[test]
    fn a_file_too_big_to_save_is_reported_and_stays_as_it_is() {
        let path = temp_file("big.bin");
        let big_file = std::fs::File::create(&path).unwrap();
        big_file.set_len(MAX_SAVED_FILE_BYTES + 1).unwrap();
        drop(big_file);
        let change = model_writes(&path, "стало");
        assert!(change.existed_before);
        assert_eq!(change.content_before, None);
        let report = restore(&[change]);
        assert_eq!(report.not_restored, vec!["big.bin"]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "стало");
    }
}
