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
    pub item_count: usize,
    pub can_go_up: bool,
    pub entries: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub size_bytes: Option<u64>,
    pub modified_unix_ms: Option<i64>,
    pub permissions: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferJob {
    pub id: String,
    pub protocol: String,
    pub direction: String,
    pub name: String,
    pub path: String,
    pub rate: Option<String>,
    pub progress_percent: Option<u8>,
    pub state: String,
}

impl AppBootstrap {
    pub fn remote_mock() -> PaneSnapshot {
        PaneSnapshot {
            id: "remote".into(),
            title: "Remote".into(),
            location: "/srv/www/releases/current".into(),
            item_count: 9,
            can_go_up: true,
            entries: vec![
                FileEntry::dir(
                    "/srv/www/releases/current/assets",
                    "assets",
                    Some(1_760_000_000_000),
                    "drwxr-xr-x",
                ),
                FileEntry::dir(
                    "/srv/www/releases/current/config",
                    "config",
                    Some(1_760_000_100_000),
                    "drwxr-x---",
                ),
                FileEntry::dir(
                    "/srv/www/releases/current/public",
                    "public",
                    Some(1_760_000_200_000),
                    "drwxr-xr-x",
                ),
                FileEntry::dir(
                    "/srv/www/releases/current/storage",
                    "storage",
                    Some(1_760_000_300_000),
                    "drwxrwx---",
                ),
                FileEntry::dir(
                    "/srv/www/releases/current/vendor",
                    "vendor",
                    Some(1_760_000_400_000),
                    "drwxr-xr-x",
                ),
                FileEntry::file(
                    "/srv/www/releases/current/.env.production",
                    ".env.production",
                    Some(1_434),
                    Some(1_760_000_500_000),
                    "-rw-------",
                ),
                FileEntry::file(
                    "/srv/www/releases/current/index.php",
                    "index.php",
                    Some(1_843),
                    Some(1_760_000_600_000),
                    "-rw-r--r--",
                ),
                FileEntry::file(
                    "/srv/www/releases/current/release.json",
                    "release.json",
                    Some(732),
                    Some(1_760_000_700_000),
                    "-rw-r--r--",
                ),
                FileEntry::symlink(
                    "/srv/www/releases/current/var",
                    "var",
                    Some(1_760_000_800_000),
                    "lrwxrwxrwx",
                ),
            ],
        }
    }

    pub fn sample_session() -> SessionSnapshot {
        SessionSnapshot {
            connection_state: "Disconnected".into(),
            protocol_mode: "SFTP primary".into(),
            host: "No active session".into(),
            auth_method: "SSH key".into(),
            trust_state: "No host selected".into(),
        }
    }

    pub fn sample_profiles() -> Vec<ConnectionProfile> {
        vec![
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
        ]
    }

    pub fn current_shortcuts() -> Vec<String> {
        vec![
            "Tab pane".into(),
            "Ctrl+1 local".into(),
            "Ctrl+2 remote".into(),
            "Ctrl+F filter".into(),
            "F5 refresh".into(),
        ]
    }
}

impl FileEntry {
    pub fn dir(path: &str, name: &str, modified_unix_ms: Option<i64>, permissions: &str) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            kind: "dir".into(),
            size_bytes: None,
            modified_unix_ms,
            permissions: permissions.into(),
        }
    }

    pub fn file(
        path: &str,
        name: &str,
        size_bytes: Option<u64>,
        modified_unix_ms: Option<i64>,
        permissions: &str,
    ) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            kind: "file".into(),
            size_bytes,
            modified_unix_ms,
            permissions: permissions.into(),
        }
    }

    pub fn symlink(
        path: &str,
        name: &str,
        modified_unix_ms: Option<i64>,
        permissions: &str,
    ) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            kind: "symlink".into(),
            size_bytes: None,
            modified_unix_ms,
            permissions: permissions.into(),
        }
    }
}
