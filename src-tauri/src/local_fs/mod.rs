use std::{
    env, fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{anyhow, bail, Context, Result};

use crate::models::{FileEntry, PaneSnapshot};

#[derive(Debug, Clone, Default)]
pub struct LocalFilesystem;

impl LocalFilesystem {
    pub fn new() -> Self {
        Self
    }

    pub fn list_directory(&self, path: Option<String>) -> Result<PaneSnapshot> {
        let resolved = self.resolve_directory(path.as_deref())?;
        self.snapshot_for_directory(&resolved)
    }

    pub fn open_directory(&self, path: &str, entry_name: &str) -> Result<PaneSnapshot> {
        let base = self.resolve_directory(Some(path))?;
        let target = base.join(entry_name);
        let target = fs::canonicalize(&target)
            .with_context(|| format!("failed to resolve path {}", target.display()))?;

        if !target.is_dir() {
            bail!("{} is not a directory", target.display());
        }

        self.snapshot_for_directory(&target)
    }

    pub fn go_up_directory(&self, path: &str) -> Result<PaneSnapshot> {
        let current = self.resolve_directory(Some(path))?;
        let Some(parent) = current.parent() else {
            return self.snapshot_for_directory(&current);
        };

        self.snapshot_for_directory(parent)
    }

    pub fn rename_entry(
        &self,
        path: &str,
        entry_name: &str,
        new_name: &str,
    ) -> Result<PaneSnapshot> {
        let base = self.resolve_directory(Some(path))?;
        let source = base.join(entry_name);

        if !source.exists() {
            bail!("{} does not exist", source.display());
        }

        if new_name.trim().is_empty() || new_name.contains('/') || new_name.contains('\0') {
            bail!("invalid target name");
        }

        let target = base.join(new_name);
        if target.exists() {
            bail!("{} already exists", target.display());
        }

        fs::rename(&source, &target).with_context(|| {
            format!(
                "failed to rename {} to {}",
                source.display(),
                target.display()
            )
        })?;

        self.snapshot_for_directory(&base)
    }

    pub fn delete_entry(&self, path: &str, entry_name: &str) -> Result<PaneSnapshot> {
        let base = self.resolve_directory(Some(path))?;
        self.delete_entry_at_path(&base, entry_name)?;

        self.snapshot_for_directory(&base)
    }

    pub fn delete_entries(&self, path: &str, entry_names: &[String]) -> Result<PaneSnapshot> {
        let base = self.resolve_directory(Some(path))?;
        for entry_name in entry_names {
            self.delete_entry_at_path(&base, entry_name)?;
        }

        self.snapshot_for_directory(&base)
    }

    fn delete_entry_at_path(&self, base: &Path, entry_name: &str) -> Result<()> {
        let target = base.join(entry_name);
        let metadata = fs::symlink_metadata(&target)
            .with_context(|| format!("failed to access {}", target.display()))?;

        if metadata.file_type().is_dir() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("failed to remove directory {}", target.display()))?;
        } else {
            fs::remove_file(&target)
                .with_context(|| format!("failed to remove file {}", target.display()))?;
        }

        Ok(())
    }

    fn resolve_directory(&self, path: Option<&str>) -> Result<PathBuf> {
        let candidate = match path {
            Some(path) if !path.trim().is_empty() => PathBuf::from(path),
            _ => env::var_os("HOME")
                .map(PathBuf::from)
                .or_else(|| env::current_dir().ok())
                .ok_or_else(|| anyhow!("failed to determine initial local directory"))?,
        };

        let canonical = fs::canonicalize(&candidate)
            .with_context(|| format!("failed to access {}", candidate.display()))?;

        if !canonical.is_dir() {
            bail!("{} is not a directory", canonical.display());
        }

        Ok(canonical)
    }

    fn snapshot_for_directory(&self, path: &Path) -> Result<PaneSnapshot> {
        let mut entries = fs::read_dir(path)
            .with_context(|| format!("failed to read directory {}", path.display()))?
            .map(|entry| self.map_entry(entry))
            .collect::<Result<Vec<_>, _>>()?;

        entries.sort_by(|left, right| {
            kind_rank(&left.kind)
                .cmp(&kind_rank(&right.kind))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(PaneSnapshot {
            id: "local".into(),
            title: "Local".into(),
            location: path.display().to_string(),
            item_count: entries.len(),
            can_go_up: path.parent().is_some(),
            entries,
            empty_message: Some("Local directory is empty.".into()),
        })
    }

    fn map_entry(&self, entry: std::io::Result<fs::DirEntry>) -> Result<FileEntry> {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let metadata = entry.metadata()?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let modified_unix_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_millis()).ok());

        let permissions = permissions_string(&metadata, &file_type);

        let mapped = if file_type.is_dir() {
            FileEntry::dir(
                &path.display().to_string(),
                &name,
                modified_unix_ms,
                &permissions,
            )
        } else if file_type.is_symlink() {
            FileEntry::symlink(
                &path.display().to_string(),
                &name,
                modified_unix_ms,
                &permissions,
            )
        } else {
            FileEntry::file(
                &path.display().to_string(),
                &name,
                Some(metadata.len()),
                modified_unix_ms,
                &permissions,
            )
        };

        Ok(mapped)
    }
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "dir" => 0,
        "symlink" => 1,
        _ => 2,
    }
}

#[cfg(unix)]
fn permissions_string(metadata: &fs::Metadata, file_type: &fs::FileType) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode();
    let mut chars = ['-'; 10];
    chars[0] = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else {
        '-'
    };

    let flags = [
        0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001,
    ];
    let marks = ['r', 'w', 'x', 'r', 'w', 'x', 'r', 'w', 'x'];

    for (index, (flag, mark)) in flags.into_iter().zip(marks).enumerate() {
        if mode & flag != 0 {
            chars[index + 1] = mark;
        }
    }

    chars.iter().collect()
}

#[cfg(not(unix))]
fn permissions_string(metadata: &fs::Metadata, file_type: &fs::FileType) -> String {
    let kind = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else {
        '-'
    };
    let readonly = if metadata.permissions().readonly() {
        "r--r--r--"
    } else {
        "rw-rw-rw-"
    };
    format!("{kind}{readonly}")
}
