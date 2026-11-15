//! Safe asynchronous filesystem primitives for the file manager.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{collections::HashMap, fs::File, io::Read};

use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use thiserror::Error;

pub mod operations;
pub use operations::{copy_path, rename_path};

/// A set of byte-identical files. The first path is kept as the suggested original.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateGroup {
    pub size: u64,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComparedItem {
    pub name: String,
    /// `somente_esquerda`, `somente_direita`, or `diferente`.
    pub difference: String,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
}

/// Searches readable text files under a directory. Files larger than 8 MiB
/// and binary/non-UTF-8 files are skipped to keep the scan predictable.
pub fn search_text_contents(root: impl AsRef<Path>, query: &str) -> Result<Vec<PathBuf>, String> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Err(format!("{} não é uma pasta acessível", root.display()));
    }
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Err("Digite o texto que deseja localizar".into());
    }
    let mut matches = Vec::new();
    for item in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !item.file_type().is_file() {
            continue;
        }
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        if metadata.len() > 8 * 1024 * 1024 {
            continue;
        }
        let Ok(contents) = std::fs::read(item.path()) else {
            continue;
        };
        if let Ok(text) = std::str::from_utf8(&contents) {
            if text.to_lowercase().contains(&query) {
                matches.push(item.path().to_path_buf());
            }
        }
    }
    matches.sort();
    Ok(matches)
}

/// Compares immediate directory contents by name, type, size, then BLAKE3 for
/// same-sized files. It does not follow symlinks or recurse into child folders.
pub fn compare_directories(
    left: impl AsRef<Path>,
    right: impl AsRef<Path>,
) -> Result<Vec<ComparedItem>, String> {
    fn read(path: &Path) -> Result<HashMap<String, (PathBuf, bool, u64)>, String> {
        let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut files = HashMap::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            files.insert(
                name,
                (
                    entry.path(),
                    file_type.is_dir(),
                    if file_type.is_dir() {
                        0
                    } else {
                        metadata.len()
                    },
                ),
            );
        }
        Ok(files)
    }
    let left = left.as_ref();
    let right = right.as_ref();
    let left_entries = read(left)?;
    let right_entries = read(right)?;
    let mut names: Vec<String> = left_entries
        .keys()
        .chain(right_entries.keys())
        .cloned()
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let mut output = Vec::new();
    for name in names {
        let l = left_entries.get(&name).or_else(|| {
            left_entries
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&name))
                .map(|(_, value)| value)
        });
        let r = right_entries.get(&name).or_else(|| {
            right_entries
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(&name))
                .map(|(_, value)| value)
        });
        let (difference, left_size, right_size) = match (l, r) {
            (Some((_, _, ls)), None) => ("somente_esquerda", Some(*ls), None),
            (None, Some((_, _, rs))) => ("somente_direita", None, Some(*rs)),
            (Some((lp, ld, ls)), Some((rp, rd, rs))) => {
                let same = ld == rd && (*ld || (ls == rs && files_equal(lp, rp)));
                (
                    if same { continue } else { "diferente" },
                    Some(*ls),
                    Some(*rs),
                )
            }
            (None, None) => continue,
        };
        output.push(ComparedItem {
            name,
            difference: difference.into(),
            left_size,
            right_size,
        });
    }
    Ok(output)
}

fn files_equal(left: &Path, right: &Path) -> bool {
    fn hash(path: &Path) -> Option<[u8; 32]> {
        let mut file = File::open(path).ok()?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).ok()?;
            if count == 0 {
                return Some(*hasher.finalize().as_bytes());
            }
            hasher.update(&buffer[..count]);
        }
    }
    hash(left).zip(hash(right)).is_some_and(|(a, b)| a == b)
}

/// Calculates recursive byte totals for each immediate child of `root`.
/// Symlinks are not followed and unreadable entries are skipped.
pub fn calculate_child_folder_sizes(root: impl AsRef<Path>) -> Result<Vec<(PathBuf, u64)>, String> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Err(format!("{} não é uma pasta acessível", root.display()));
    }
    let mut sizes: HashMap<PathBuf, u64> = HashMap::new();
    for item in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !item.file_type().is_file() {
            continue;
        }
        let Ok(relative) = item.path().strip_prefix(root) else {
            continue;
        };
        let Some(component) = relative.components().next() else {
            continue;
        };
        let child = root.join(component.as_os_str());
        let bytes = item.metadata().map(|m| m.len()).unwrap_or(0);
        *sizes.entry(child).or_default() += bytes;
    }
    let mut result: Vec<_> = sizes.into_iter().collect();
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Finds content-identical files. Size is used to reduce hashing work; BLAKE3
/// is then computed in a fixed-size buffer, keeping memory use bounded.
pub fn find_duplicates(root: impl AsRef<Path>) -> Result<Vec<DuplicateGroup>, String> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Err(format!("{} não é uma pasta acessível", root.display()));
    }
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for item in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !item.file_type().is_file() {
            continue;
        }
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        by_size
            .entry(metadata.len())
            .or_default()
            .push(item.path().to_path_buf());
    }

    let mut groups = Vec::new();
    for (size, candidates) in by_size.into_iter().filter(|(_, paths)| paths.len() > 1) {
        let mut by_hash: HashMap<[u8; 32], Vec<PathBuf>> = HashMap::new();
        for path in candidates {
            let Ok(mut file) = File::open(&path) else {
                continue;
            };
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0u8; 64 * 1024];
            let mut failed = false;
            loop {
                let count = match file.read(&mut buffer) {
                    Ok(count) => count,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
            if failed {
                continue;
            }
            by_hash
                .entry(*hasher.finalize().as_bytes())
                .or_default()
                .push(path);
        }
        for mut files in by_hash.into_values().filter(|paths| paths.len() > 1) {
            files.sort();
            groups.push(DuplicateGroup { size, files });
        }
    }
    groups.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(groups)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// Per-entry error so a locked or disappearing item does not hide siblings.
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum ListDirectoryError {
    #[error("could not open directory {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Lists one directory without recursively traversing it.
///
/// Tokio's filesystem API keeps blocking filesystem calls off the async executor.
/// Individual metadata failures are returned on their corresponding entry; an
/// error opening the requested directory fails the whole operation.
pub async fn list_directory(
    path: impl AsRef<Path>,
) -> Result<Vec<DirectoryEntry>, ListDirectoryError> {
    let path = path.as_ref().to_path_buf();
    let mut reader =
        tokio::fs::read_dir(&path)
            .await
            .map_err(|source| ListDirectoryError::Open {
                path: path.clone(),
                source,
            })?;

    let mut paths = Vec::new();
    while let Some(entry) =
        reader
            .next_entry()
            .await
            .map_err(|source| ListDirectoryError::Open {
                path: path.clone(),
                source,
            })?
    {
        paths.push((
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
        ));
    }

    // Bound concurrent metadata requests to keep latency low without flooding
    // the OS thread pool when a folder contains hundreds of thousands of items.
    let mut entries = stream::iter(paths)
        .map(|(name, entry_path)| async move {
            match tokio::fs::symlink_metadata(&entry_path).await {
                Ok(metadata) => {
                    let is_hidden = metadata_is_hidden(&metadata, &entry_path);
                    DirectoryEntry {
                        name,
                        path: entry_path,
                        is_dir: metadata.is_dir(),
                        is_symlink: metadata.file_type().is_symlink(),
                        is_hidden,
                        size: (!metadata.is_dir()).then_some(metadata.len()),
                        modified: metadata.modified().ok(),
                        created: metadata.created().ok(),
                        error: None,
                    }
                }
                Err(error) => DirectoryEntry {
                    name,
                    path: entry_path,
                    is_dir: false,
                    is_symlink: false,
                    is_hidden: false,
                    size: None,
                    modified: None,
                    created: None,
                    error: Some(error.to_string()),
                },
            }
        })
        .buffer_unordered(64)
        .collect::<Vec<_>>()
        .await;

    // Stable, predictable ordering; directories first, then case-insensitive name.
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

#[cfg(windows)]
fn metadata_is_hidden(metadata: &std::fs::Metadata, _path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
fn metadata_is_hidden(_metadata: &std::fs::Metadata, path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}
