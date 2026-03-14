use anyhow::{Context, Result, anyhow};
use russh_sftp::{client::SftpSession, protocol::FileType};

use crate::models::{AppBootstrap, FileEntry, PaneSnapshot};

pub struct RemoteSftpEngine;

impl RemoteSftpEngine {
    pub async fn list_directory(sftp: &SftpSession, path: Option<&str>) -> Result<PaneSnapshot> {
        let requested = path.unwrap_or(".");
        let operation_path = operation_path(requested);

        let mut entries = sftp
            .read_dir(&operation_path)
            .await
            .with_context(|| friendly_path_error("list", &operation_path))?
            .map(|entry| map_entry(&operation_path, entry))
            .collect::<Result<Vec<_>>>()?;

        entries.sort_by(|left, right| sort_key(left).cmp(&sort_key(right)));

        let location = display_path(sftp, requested, &operation_path).await;

        Ok(PaneSnapshot {
            id: "remote".into(),
            title: "Remote".into(),
            location: location.clone(),
            item_count: entries.len(),
            can_go_up: location != "/",
            empty_message: Some("Remote directory is empty.".into()),
            entries,
        })
    }

    pub fn placeholder() -> PaneSnapshot {
        AppBootstrap::remote_placeholder()
    }

    pub fn parent_path(path: &str) -> Option<String> {
        let canonical = normalize_remote_path(path);
        if canonical == "/" {
            return None;
        }

        let mut segments = canonical
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        segments.pop();

        if segments.is_empty() {
            Some("/".into())
        } else {
            Some(format!("/{}", segments.join("/")))
        }
    }
}

fn friendly_path_error(action: &str, path: &str) -> String {
    match action {
        "open" => format!("Unable to open remote path `{path}`."),
        _ => format!("Unable to list remote directory `{path}`."),
    }
}

async fn display_path(sftp: &SftpSession, requested: &str, operation_path: &str) -> String {
    match sftp.canonicalize(requested).await {
        Ok(path) => path,
        Err(_) => operation_path.to_string(),
    }
}

fn operation_path(requested: &str) -> String {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        ".".into()
    } else {
        trimmed.into()
    }
}

fn map_entry(parent: &str, entry: russh_sftp::client::fs::DirEntry) -> Result<FileEntry> {
    let name = entry.file_name();
    let metadata = entry.metadata();
    let path = join_remote_path(parent, &name);
    let modified_unix_ms = metadata.mtime.map(|value| i64::from(value) * 1000);
    let permissions = format_permissions(&metadata);

    let file_entry = match entry.file_type() {
        FileType::Dir => FileEntry::dir(&path, &name, modified_unix_ms, &permissions),
        FileType::Symlink => FileEntry::symlink(&path, &name, modified_unix_ms, &permissions),
        FileType::File => FileEntry::file(
            &path,
            &name,
            metadata.size,
            modified_unix_ms,
            &permissions,
        ),
        _ => FileEntry::file(&path, &name, metadata.size, modified_unix_ms, &permissions),
    };

    Ok(file_entry)
}

fn sort_key(entry: &FileEntry) -> (u8, String) {
    let bucket = match entry.kind.as_str() {
        "dir" => 0,
        "symlink" => 1,
        _ => 2,
    };

    (bucket, entry.name.to_lowercase())
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

fn normalize_remote_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".into();
    }

    format!("/{}", trimmed.trim_matches('/'))
}

fn format_permissions(metadata: &russh_sftp::client::fs::Metadata) -> String {
    let kind = if metadata.is_dir() {
        'd'
    } else if metadata.is_symlink() {
        'l'
    } else {
        '-'
    };

    format!("{kind}{}", metadata.permissions())
}

pub fn require_directory(entry_kind: &str) -> Result<()> {
    if entry_kind == "dir" {
        Ok(())
    } else {
        Err(anyhow!("remote entry is not a directory"))
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteSftpEngine;

    #[test]
    fn parent_path_handles_root_and_nested_directories() {
        assert_eq!(RemoteSftpEngine::parent_path("/"), None);
        assert_eq!(RemoteSftpEngine::parent_path("/srv"), Some("/".into()));
        assert_eq!(RemoteSftpEngine::parent_path("/srv/www/releases"), Some("/srv/www".into()));
    }
}
