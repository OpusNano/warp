use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub connection_profiles: Vec<ConnectionProfile>,
    pub session: SessionSnapshot,
    pub panes: PaneSet,
    pub transfers: Vec<TransferJob>,
    pub shortcuts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub name: String,
    pub target: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub connection_state: String,
    pub protocol_mode: String,
    pub host: String,
    pub auth_method: String,
    pub trust_state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneSet {
    pub local: PaneSnapshot,
    pub remote: PaneSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSnapshot {
    pub id: String,
    pub title: String,
    pub location: String,
    pub filter: String,
    pub item_count: usize,
    pub selected_count: usize,
    pub entries: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub kind: String,
    pub size: String,
    pub modified: String,
    pub permissions: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferJob {
    pub id: String,
    pub protocol: String,
    pub direction: String,
    pub name: String,
    pub path: String,
    pub rate: String,
    pub progress: String,
    pub state: String,
}

impl AppBootstrap {
    pub fn sample() -> Self {
        Self {
            connection_profiles: vec![
                ConnectionProfile {
                    name: "prod-edge".into(),
                    target: "deploy@edge-01.example.com:22".into(),
                    auth: "ed25519".into(),
                },
                ConnectionProfile {
                    name: "media-origin".into(),
                    target: "ops@origin.internal:22".into(),
                    auth: "password".into(),
                },
            ],
            session: SessionSnapshot {
                connection_state: "Connected".into(),
                protocol_mode: "SFTP primary".into(),
                host: "deploy@edge-01.example.com".into(),
                auth_method: "SSH key".into(),
                trust_state: "Known host verified".into(),
            },
            panes: PaneSet {
                local: PaneSnapshot {
                    id: "local".into(),
                    title: "Local".into(),
                    location: "/home/cyberdyne/projects/warp".into(),
                    filter: String::new(),
                    item_count: 8,
                    selected_count: 2,
                    entries: vec![
                        FileEntry::dir("src", "Today 17:34", "drwxr-xr-x"),
                        FileEntry::dir("src-tauri", "Today 18:19", "drwxr-xr-x"),
                        FileEntry::file("package.json", "841 B", "Today 18:18", "-rw-r--r--"),
                        FileEntry::file("package-lock.json", "42 KB", "Today 18:18", "-rw-r--r--"),
                        FileEntry::file("vite.config.ts", "196 B", "Today 18:18", "-rw-r--r--"),
                        FileEntry::file("tsconfig.app.json", "689 B", "Today 18:17", "-rw-r--r--"),
                        FileEntry::dir(".github", "Today 18:26", "drwxr-xr-x"),
                        FileEntry::file("README.md", "3.1 KB", "Today 18:26", "-rw-r--r--"),
                    ],
                },
                remote: PaneSnapshot {
                    id: "remote".into(),
                    title: "Remote".into(),
                    location: "/srv/www/releases/current".into(),
                    filter: String::new(),
                    item_count: 9,
                    selected_count: 1,
                    entries: vec![
                        FileEntry::dir("assets", "Today 17:12", "drwxr-xr-x"),
                        FileEntry::dir("config", "Today 17:15", "drwxr-x---"),
                        FileEntry::dir("public", "Today 17:09", "drwxr-xr-x"),
                        FileEntry::dir("storage", "Today 16:58", "drwxrwx---"),
                        FileEntry::file(".env.production", "1.4 KB", "Today 16:40", "-rw-------"),
                        FileEntry::file("index.php", "1.8 KB", "Today 17:20", "-rw-r--r--"),
                        FileEntry::file("release.json", "732 B", "Today 16:55", "-rw-r--r--"),
                        FileEntry::symlink("var", "Today 16:58", "lrwxrwxrwx"),
                        FileEntry::dir("vendor", "Today 16:49", "drwxr-xr-x"),
                    ],
                },
            },
            transfers: vec![
                TransferJob {
                    id: "1".into(),
                    protocol: "SFTP".into(),
                    direction: "Upload".into(),
                    name: "release.json".into(),
                    path: "/home/cyberdyne/projects/warp/release.json -> /srv/www/releases/current/release.json".into(),
                    rate: "19.4 MB/s".into(),
                    progress: "81%".into(),
                    state: "Running".into(),
                },
                TransferJob {
                    id: "2".into(),
                    protocol: "SFTP".into(),
                    direction: "Download".into(),
                    name: "logs-2026-03-13.tar.gz".into(),
                    path: "/srv/backups/logs-2026-03-13.tar.gz -> /home/cyberdyne/Downloads".into(),
                    rate: "Completed".into(),
                    progress: "100%".into(),
                    state: "Complete".into(),
                },
                TransferJob {
                    id: "3".into(),
                    protocol: "SCP compatibility".into(),
                    direction: "Upload".into(),
                    name: "hotfix.patch".into(),
                    path: "/home/cyberdyne/projects/warp/hotfix.patch -> /tmp/hotfix.patch".into(),
                    rate: "Queued".into(),
                    progress: "0%".into(),
                    state: "Queued".into(),
                },
            ],
            shortcuts: vec![
                "Tab pane".into(),
                "Ctrl+L path".into(),
                "Ctrl+F filter".into(),
                "F2 rename".into(),
                "Delete remove".into(),
            ],
        }
    }
}

impl FileEntry {
    fn dir(name: &str, modified: &str, permissions: &str) -> Self {
        Self {
            name: name.into(),
            kind: "dir".into(),
            size: String::new(),
            modified: modified.into(),
            permissions: permissions.into(),
        }
    }

    fn file(name: &str, size: &str, modified: &str, permissions: &str) -> Self {
        Self {
            name: name.into(),
            kind: "file".into(),
            size: size.into(),
            modified: modified.into(),
            permissions: permissions.into(),
        }
    }

    fn symlink(name: &str, modified: &str, permissions: &str) -> Self {
        Self {
            name: name.into(),
            kind: "symlink".into(),
            size: String::new(),
            modified: modified.into(),
            permissions: permissions.into(),
        }
    }
}
