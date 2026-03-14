use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub connection_profiles: Vec<ConnectionProfile>,
    pub session: SessionSnapshot,
    pub panes: PaneSet,
    pub transfers: TransferQueueSnapshot,
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
    pub last_error: Option<String>,
    pub can_disconnect: bool,
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
    pub empty_message: Option<String>,
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
    pub source_path: String,
    pub destination_path: String,
    pub rate: Option<String>,
    pub bytes_total: Option<u64>,
    pub bytes_transferred: u64,
    pub progress_percent: Option<u8>,
    pub state: String,
    pub error_message: Option<String>,
    pub conflict: Option<TransferConflict>,
    pub can_cancel: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferConflict {
    pub destination_exists: bool,
    pub destination_kind: String,
    pub can_overwrite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferQueueSnapshot {
    pub jobs: Vec<TransferJob>,
    pub active_job_id: Option<String>,
    pub queued_count: usize,
    pub finished_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustPrompt {
    pub host: String,
    pub port: u16,
    pub key_algorithm: String,
    pub fingerprint_sha256: String,
    pub status: String,
    pub message: String,
    pub expected_fingerprint_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConnectionSnapshot {
    pub session: SessionSnapshot,
    pub remote_pane: PaneSnapshot,
    pub trust_prompt: Option<TrustPrompt>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: ConnectAuth,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConnectAuth {
    Password {
        password: String,
    },
    Key {
        #[serde(rename = "privateKeyPath")]
        private_key_path: String,
        passphrase: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustDecision {
    pub trust: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueDownloadRequest {
    pub remote_path: String,
    pub remote_name: String,
    pub local_directory: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueUploadRequest {
    pub local_path: String,
    pub local_name: String,
    pub remote_directory: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferConflictResolution {
    pub action: String,
}

impl AppBootstrap {
    pub fn empty_transfers() -> TransferQueueSnapshot {
        TransferQueueSnapshot {
            jobs: Vec::new(),
            active_job_id: None,
            queued_count: 0,
            finished_count: 0,
        }
    }

    pub fn remote_placeholder() -> PaneSnapshot {
        PaneSnapshot {
            id: "remote".into(),
            title: "Remote".into(),
            location: "Not connected".into(),
            item_count: 0,
            can_go_up: false,
            entries: Vec::new(),
            empty_message: Some("Connect to a host to browse remote files.".into()),
        }
    }

    pub fn sample_session() -> SessionSnapshot {
        SessionSnapshot {
            connection_state: "Disconnected".into(),
            protocol_mode: "SFTP primary".into(),
            host: "No active session".into(),
            auth_method: "None".into(),
            trust_state: "No host selected".into(),
            last_error: None,
            can_disconnect: false,
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

impl RemoteConnectionSnapshot {
    pub fn disconnected(session: SessionSnapshot) -> Self {
        Self {
            session,
            remote_pane: AppBootstrap::remote_placeholder(),
            trust_prompt: None,
        }
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
