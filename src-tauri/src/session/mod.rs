use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use russh::{
    Disconnect, client,
    keys::{Algorithm, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey},
};
use russh_sftp::client::SftpSession;
use tauri::{AppHandle, Emitter};
use tokio::{sync::Mutex, time};

use crate::{
    events::REMOTE_SESSION_UPDATED_EVENT,
    models::{
        AppBootstrap, ConnectAuth, ConnectRequest, RemoteConnectionSnapshot, SessionSnapshot,
        TrustDecision, TrustPrompt,
    },
    remote_sftp::RemoteSftpEngine,
    trust::{TrustCheck, TrustModel, fingerprint_sha256},
};

pub struct SessionManager {
    app_handle: AppHandle,
    trust_model: TrustModel,
    next_session_id: AtomicU64,
    state: Mutex<SessionState>,
}

struct SessionState {
    active: Option<ActiveRemoteSession>,
    pending_trust: Option<PendingTrust>,
    last_error: Option<String>,
}

struct ActiveRemoteSession {
    id: u64,
    request: ConnectRequest,
    handle: client::Handle<SshClientHandler>,
    sftp: SftpSession,
    remote_pane: crate::models::PaneSnapshot,
    trust_state: String,
}

struct PendingTrust {
    request: ConnectRequest,
    server_key: PublicKey,
    prompt: TrustPrompt,
}

#[derive(Clone)]
enum ServerKeyPolicy {
    VerifyStored,
    AcceptExact(PublicKey),
}

#[derive(Clone, Default)]
struct ServerKeyCapture {
    observed: Option<ObservedServerKey>,
}

#[derive(Clone)]
struct ObservedServerKey {
    public_key: PublicKey,
    fingerprint_sha256: String,
    algorithm: String,
}

struct SshClientHandler {
    trust_model: TrustModel,
    host: String,
    port: u16,
    policy: ServerKeyPolicy,
    capture: Arc<Mutex<ServerKeyCapture>>,
}

impl SessionManager {
    pub fn new(app_handle: AppHandle, base_dir: std::path::PathBuf) -> Self {
        Self {
            app_handle,
            trust_model: TrustModel::new(base_dir),
            next_session_id: AtomicU64::new(1),
            state: Mutex::new(SessionState {
                active: None,
                pending_trust: None,
                last_error: None,
            }),
        }
    }

    pub async fn connect(self: &Arc<Self>, request: ConnectRequest) -> Result<RemoteConnectionSnapshot> {
        self.disconnect_internal().await;

        match self
            .connect_with_policy(request.clone(), ServerKeyPolicy::VerifyStored)
            .await
        {
            Ok(active) => {
                let active_id = active.id;
                let mut state = self.state.lock().await;
                state.last_error = None;
                state.pending_trust = None;
                state.active = Some(active);
                let snapshot = snapshot_from_state(&state);
                drop(state);
                self.spawn_liveness_monitor(active_id);
                Ok(snapshot)
            }
            Err(ConnectOutcome::TrustRequired { prompt, server_key }) => {
                let mut state = self.state.lock().await;
                state.active = None;
                state.last_error = None;
                state.pending_trust = Some(PendingTrust {
                    request,
                    server_key,
                    prompt,
                });
                Ok(snapshot_from_state(&state))
            }
            Err(ConnectOutcome::Failed(message)) => {
                let mut state = self.state.lock().await;
                state.active = None;
                state.pending_trust = None;
                state.last_error = Some(message);
                Ok(snapshot_from_state(&state))
            }
        }
    }

    pub async fn snapshot(self: &Arc<Self>) -> RemoteConnectionSnapshot {
        let state = self.state.lock().await;
        snapshot_from_state(&state)
    }

    pub async fn resolve_trust(self: &Arc<Self>, decision: TrustDecision) -> Result<RemoteConnectionSnapshot> {
        let pending = {
            let mut state = self.state.lock().await;
            state.active = None;
            state.last_error = None;
            state.pending_trust.take()
        };

        let Some(pending) = pending else {
            bail!("There is no pending host trust decision.")
        };

        if !decision.trust {
            let mut state = self.state.lock().await;
            state.last_error = Some("Connection cancelled before trust was granted.".into());
            return Ok(snapshot_from_state(&state));
        }

        if pending.prompt.status != "firstSeen" {
            let mut state = self.state.lock().await;
            state.last_error = Some("The stored host fingerprint does not match this server. Review the trust store before trying again.".into());
            return Ok(snapshot_from_state(&state));
        }

        self.trust_model
            .remember_host(&pending.request.host, pending.request.port, &pending.server_key)
            .context("failed to persist trusted host")?;

        match self
            .connect_with_policy(
                pending.request.clone(),
                ServerKeyPolicy::AcceptExact(pending.server_key),
            )
            .await
        {
            Ok(active) => {
                let active_id = active.id;
                let mut state = self.state.lock().await;
                state.last_error = None;
                state.pending_trust = None;
                state.active = Some(active);
                let snapshot = snapshot_from_state(&state);
                drop(state);
                self.spawn_liveness_monitor(active_id);
                Ok(snapshot)
            }
            Err(ConnectOutcome::TrustRequired { .. }) => {
                let mut state = self.state.lock().await;
                state.last_error = Some("The server fingerprint changed while confirming trust. Try connecting again and verify the host carefully.".into());
                Ok(snapshot_from_state(&state))
            }
            Err(ConnectOutcome::Failed(message)) => {
                let mut state = self.state.lock().await;
                state.last_error = Some(message);
                Ok(snapshot_from_state(&state))
            }
        }
    }

    pub async fn disconnect(self: &Arc<Self>) -> Result<RemoteConnectionSnapshot> {
        self.disconnect_internal().await;
        let mut state = self.state.lock().await;
        state.pending_trust = None;
        state.last_error = None;
        Ok(snapshot_from_state(&state))
    }

    pub async fn refresh_remote_directory(self: &Arc<Self>) -> Result<RemoteConnectionSnapshot> {
        let mut state = self.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            state.last_error = Some("Connect to a host before refreshing the remote pane.".into());
            return Ok(snapshot_from_state(&state));
        };

        match RemoteSftpEngine::list_directory(&active.sftp, Some(&active.remote_pane.location)).await {
            Ok(next_pane) => {
                active.remote_pane = next_pane;
                state.last_error = None;
                Ok(snapshot_from_state(&state))
            }
            Err(error) => {
                apply_remote_operation_error(&mut state, "refresh the remote directory", error);
                Ok(snapshot_from_state(&state))
            }
        }
    }

    pub async fn open_remote_directory(self: &Arc<Self>, path: String) -> Result<RemoteConnectionSnapshot> {
        let mut state = self.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            state.last_error = Some("Connect to a host before opening a remote directory.".into());
            return Ok(snapshot_from_state(&state));
        };

        match RemoteSftpEngine::list_directory(&active.sftp, Some(&path)).await {
            Ok(next_pane) => {
                active.remote_pane = next_pane;
                state.last_error = None;
                Ok(snapshot_from_state(&state))
            }
            Err(error) => {
                apply_remote_operation_error(&mut state, "open the remote directory", error);
                Ok(snapshot_from_state(&state))
            }
        }
    }

    pub async fn go_up_remote_directory(self: &Arc<Self>) -> Result<RemoteConnectionSnapshot> {
        let mut state = self.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            state.last_error = Some("Connect to a host before navigating the remote pane.".into());
            return Ok(snapshot_from_state(&state));
        };
        let Some(parent) = RemoteSftpEngine::parent_path(&active.remote_pane.location) else {
            state.last_error = None;
            return Ok(snapshot_from_state(&state));
        };

        match RemoteSftpEngine::list_directory(&active.sftp, Some(&parent)).await {
            Ok(next_pane) => {
                active.remote_pane = next_pane;
                state.last_error = None;
                Ok(snapshot_from_state(&state))
            }
            Err(error) => {
                apply_remote_operation_error(&mut state, "open the parent remote directory", error);
                Ok(snapshot_from_state(&state))
            }
        }
    }

    pub async fn open_transfer_sftp(self: &Arc<Self>) -> Result<SftpSession> {
        let mut state = self.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            bail!("Connect to a host before starting a transfer.")
        };

        let channel = match active
            .handle
            .channel_open_session()
            .await
        {
            Ok(channel) => channel,
            Err(_) => {
                let message = if active.handle.is_closed() {
                    "The SSH session ended before the transfer could start. Reconnect and try again."
                } else {
                    "Connected to the server, but could not open an SSH session channel for transfer."
                };
                if active.handle.is_closed() {
                    state.active = None;
                    state.pending_trust = None;
                    state.last_error = Some(message.into());
                    let snapshot = snapshot_from_state(&state);
                    drop(state);
                    self.emit_snapshot(snapshot);
                }
                bail!(message)
            }
        };

        if let Err(_) = channel
            .request_subsystem(true, "sftp")
            .await
        {
            bail!("Connected to the server, but SFTP is not available for transfers on this host.")
        }

        SftpSession::new(channel.into_stream())
            .await
            .map_err(|_| anyhow!("Connected to the server, but could not start an SFTP transfer channel."))
    }

    pub async fn handle_connection_loss(self: &Arc<Self>, message: String) -> RemoteConnectionSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            if state.active.is_none() && state.last_error.as_deref() == Some(message.as_str()) {
                return snapshot_from_state(&state);
            }

            state.active = None;
            state.pending_trust = None;
            state.last_error = Some(message);
            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        snapshot
    }

    fn spawn_liveness_monitor(self: &Arc<Self>, session_id: u64) {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                let snapshot = {
                    let mut state = manager.state.lock().await;
                    let Some(active) = state.active.as_ref() else {
                        return;
                    };

                    if active.id != session_id {
                        return;
                    }

                    if !active.handle.is_closed() {
                        None
                    } else {
                        state.active = None;
                        state.pending_trust = None;
                        state.last_error = Some(
                            "Connection lost after idle. Reconnect to continue browsing or transferring.".into(),
                        );
                        Some(snapshot_from_state(&state))
                    }
                };

                if let Some(snapshot) = snapshot {
                    manager.emit_snapshot(snapshot);
                    return;
                }
            }
        });
    }

    fn emit_snapshot(&self, snapshot: RemoteConnectionSnapshot) {
        let _ = self.app_handle.emit(REMOTE_SESSION_UPDATED_EVENT, snapshot);
    }

    async fn disconnect_internal(&self) {
        let active = {
            let mut state = self.state.lock().await;
            state.pending_trust = None;
            state.active.take()
        };

        if let Some(active) = active {
            let _ = active.sftp.close().await;
            let _ = active
                .handle
                .disconnect(Disconnect::ByApplication, "", "en")
                .await;
        }
    }

    async fn connect_with_policy(
        &self,
        request: ConnectRequest,
        policy: ServerKeyPolicy,
    ) -> std::result::Result<ActiveRemoteSession, ConnectOutcome> {
        let capture = Arc::new(Mutex::new(ServerKeyCapture::default()));
        let handler = SshClientHandler {
            trust_model: self.trust_model.clone(),
            host: request.host.clone(),
            port: request.port,
            policy,
            capture: capture.clone(),
        };

        let config = client::Config {
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(45)),
            keepalive_max: 3,
            ..Default::default()
        };

        let mut handle = match client::connect(Arc::new(config), (&request.host[..], request.port), handler).await {
            Ok(handle) => handle,
            Err(error) => {
                return Err(
                    map_connect_error(&self.trust_model, &request.host, request.port, error, &capture)
                        .await,
                )
            }
        };

        authenticate(&mut handle, &request)
            .await
            .map_err(|error| ConnectOutcome::Failed(friendly_auth_error(&request, &error)))?;

        let channel = handle
            .channel_open_session()
            .await
            .map_err(|_| ConnectOutcome::Failed("Connected to the server, but could not open an SSH session channel.".into()))?;

        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|_| ConnectOutcome::Failed("Connected to the server, but SFTP is not available for this account or host.".into()))?;

        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|_| ConnectOutcome::Failed("Connected to the server, but could not start the SFTP session.".into()))?;

        let remote_pane = RemoteSftpEngine::list_directory(&sftp, None)
            .await
            .map_err(|error| ConnectOutcome::Failed(error.to_string()))?;

        let captured_key = captured_public_key(&capture)
            .await
            .map_err(|error| ConnectOutcome::Failed(error.to_string()))?;

        let trust_state = match self
            .trust_model
            .verify_host(&request.host, request.port, &captured_key)
            .map_err(|error| ConnectOutcome::Failed(error.to_string()))?
        {
            TrustCheck::Verified(_) => "Known host verified".to_string(),
            TrustCheck::Unknown => "Trusted this session".to_string(),
            TrustCheck::Mismatch(_) => "Host key mismatch".to_string(),
        };

        Ok(ActiveRemoteSession {
            id: self.next_session_id.fetch_add(1, Ordering::Relaxed),
            request,
            handle,
            sftp,
            remote_pane,
            trust_state,
        })
    }
}

impl client::Handler for SshClientHandler {
    type Error = anyhow::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let trust_model = self.trust_model.clone();
        let host = self.host.clone();
        let port = self.port;
        let policy = self.policy.clone();
        let capture = self.capture.clone();
        let server_public_key = server_public_key.clone();

        async move {
            {
                let mut observed = capture.lock().await;
                observed.observed = Some(ObservedServerKey {
                    fingerprint_sha256: fingerprint_sha256(&server_public_key),
                    algorithm: server_public_key.algorithm().to_string(),
                    public_key: server_public_key.clone(),
                });
            }

            let decision = match policy {
                ServerKeyPolicy::AcceptExact(expected) => expected == server_public_key,
                ServerKeyPolicy::VerifyStored => {
                    matches!(trust_model.verify_host(&host, port, &server_public_key)?, TrustCheck::Verified(_))
                }
            };

            Ok(decision)
        }
    }
}

async fn authenticate(handle: &mut client::Handle<SshClientHandler>, request: &ConnectRequest) -> Result<()> {
    match &request.auth {
        ConnectAuth::Password { password } => {
            let result = handle.authenticate_password(&request.username, password).await?;
            if !result.success() {
                bail!("SSH authentication failed for {}@{}", request.username, request.host);
            }
        }
        ConnectAuth::Key {
            private_key_path,
            passphrase,
        } => {
            let private_key = load_private_key(private_key_path, passphrase.as_deref())?;
            let hash_alg = if matches!(private_key.algorithm(), Algorithm::Rsa { .. }) {
                handle.best_supported_rsa_hash().await?.flatten().or(Some(HashAlg::Sha256))
            } else {
                None
            };

            let result = handle
                .authenticate_publickey(
                    &request.username,
                    PrivateKeyWithHashAlg::new(Arc::new(private_key), hash_alg),
                )
                .await?;

            if !result.success() {
                bail!("SSH key authentication failed for {}@{}", request.username, request.host);
            }
        }
    }

    Ok(())
}

fn load_private_key(path: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
    russh::keys::load_secret_key(Path::new(path), passphrase)
        .with_context(|| format!("failed to load SSH private key from {path}"))
}

async fn map_connect_error(
    trust_model: &TrustModel,
    host: &str,
    port: u16,
    error: anyhow::Error,
    capture: &Arc<Mutex<ServerKeyCapture>>,
) -> ConnectOutcome {
    match capture.lock().await.observed.clone() {
        Some(observed) => match trust_model.verify_host(host, port, &observed.public_key) {
            Ok(TrustCheck::Unknown) => ConnectOutcome::TrustRequired {
                prompt: TrustPrompt {
                    host: host.into(),
                    port,
                    key_algorithm: observed.algorithm,
                    fingerprint_sha256: observed.fingerprint_sha256,
                    status: "firstSeen".into(),
                    message: "First time seeing this host. Verify the fingerprint before connecting.".into(),
                    expected_fingerprint_sha256: None,
                },
                server_key: observed.public_key,
            },
            Ok(TrustCheck::Mismatch(entry)) => ConnectOutcome::TrustRequired {
                prompt: TrustPrompt {
                    host: host.into(),
                    port,
                    key_algorithm: observed.algorithm,
                    fingerprint_sha256: observed.fingerprint_sha256,
                    status: "mismatch".into(),
                    message: "Known host fingerprint mismatch. Connection is blocked until the trust store is corrected.".into(),
                    expected_fingerprint_sha256: Some(entry.fingerprint_sha256),
                },
                server_key: observed.public_key,
            },
            Ok(TrustCheck::Verified(_)) => ConnectOutcome::Failed(friendly_connect_error(error)),
            Err(store_error) => ConnectOutcome::Failed(format!("Unable to read the stored host fingerprints: {store_error}")),
        },
        None => ConnectOutcome::Failed(friendly_connect_error(error)),
    }
}

async fn captured_public_key(capture: &Arc<Mutex<ServerKeyCapture>>) -> Result<PublicKey> {
    capture
        .lock()
        .await
        .observed
        .as_ref()
        .map(|observed| observed.public_key.clone())
        .ok_or_else(|| anyhow!("server key was not captured during handshake"))
}

fn snapshot_from_state(state: &SessionState) -> RemoteConnectionSnapshot {
    if let Some(active) = &state.active {
        return RemoteConnectionSnapshot {
            session: SessionSnapshot {
                connection_state: "Connected".into(),
                protocol_mode: "SFTP primary".into(),
                host: format!("{}@{}:{}", active.request.username, active.request.host, active.request.port),
                auth_method: auth_method_label(&active.request.auth),
                trust_state: active.trust_state.clone(),
                last_error: state.last_error.clone(),
                can_disconnect: true,
            },
            remote_pane: active.remote_pane.clone(),
            trust_prompt: None,
        };
    }

    if let Some(pending) = &state.pending_trust {
        return RemoteConnectionSnapshot {
            session: SessionSnapshot {
                connection_state: "Awaiting trust".into(),
                protocol_mode: "SFTP primary".into(),
                host: format!("{}@{}:{}", pending.request.username, pending.request.host, pending.request.port),
                auth_method: auth_method_label(&pending.request.auth),
                trust_state: if pending.prompt.status == "mismatch" {
                    "Host key mismatch".into()
                } else {
                    "First-seen host".into()
                },
                last_error: state.last_error.clone(),
                can_disconnect: false,
            },
            remote_pane: AppBootstrap::remote_placeholder(),
            trust_prompt: Some(pending.prompt.clone()),
        };
    }

    RemoteConnectionSnapshot::disconnected(SessionSnapshot {
        connection_state: "Disconnected".into(),
        protocol_mode: "SFTP primary".into(),
        host: "No active session".into(),
        auth_method: "None".into(),
        trust_state: "No host selected".into(),
        last_error: state.last_error.clone(),
        can_disconnect: false,
    })
}

fn auth_method_label(auth: &ConnectAuth) -> String {
    match auth {
        ConnectAuth::Password { .. } => "Password".into(),
        ConnectAuth::Key { .. } => "SSH key".into(),
    }
}

enum ConnectOutcome {
    TrustRequired {
        prompt: TrustPrompt,
        server_key: PublicKey,
    },
    Failed(String),
}

fn friendly_connect_error(error: anyhow::Error) -> String {
    let message = error.to_string().to_lowercase();

    if message.contains("dns") || message.contains("name or service not known") || message.contains("failed to lookup") {
        return "Could not resolve that host name.".into();
    }

    if message.contains("connection refused") {
        return "The SSH server refused the connection.".into();
    }

    if message.contains("timed out") {
        return "Timed out while connecting to the SSH server.".into();
    }

    if message.contains("network is unreachable") || message.contains("no route to host") {
        return "The SSH server could not be reached from this machine.".into();
    }

    "Unable to connect to the SSH server.".into()
}

fn friendly_auth_error(request: &ConnectRequest, error: &anyhow::Error) -> String {
    let message = error.to_string().to_lowercase();

    match &request.auth {
        ConnectAuth::Password { .. } => {
            if message.contains("auth") || message.contains("password") {
                return "SSH password authentication was rejected. Check the username and password.".into();
            }
        }
        ConnectAuth::Key {
            private_key_path,
            passphrase: _,
        } => {
            if message.contains("decrypt") || message.contains("passphrase") {
                return "The SSH key could not be unlocked. Check the key passphrase.".into();
            }

            if message.contains("no such file") {
                return format!("SSH key file not found: {private_key_path}");
            }

            if message.contains("permission denied") {
                return format!("The SSH key file could not be read: {private_key_path}");
            }

            if message.contains("auth") || message.contains("publickey") {
                return "SSH key authentication was rejected. Check the username and private key.".into();
            }
        }
    }

    "SSH authentication failed.".into()
}

fn apply_remote_operation_error(state: &mut SessionState, action: &str, error: anyhow::Error) {
    let message = error.to_string().to_lowercase();

    if message.contains("disconnect")
        || message.contains("channel closed")
        || message.contains("session closed")
        || message.contains("broken pipe")
        || message.contains("connection reset")
        || message.contains("unexpected eof")
    {
        state.active = None;
        state.pending_trust = None;
        state.last_error = Some(format!("The SSH session ended while trying to {action}. Reconnect and try again."));
        return;
    }

    state.last_error = Some(match message.as_str() {
        _ if message.contains("permission denied") => format!("Permission denied while trying to {action}."),
        _ if message.contains("no such file") => format!("The remote path is no longer available, so Warp could not {action}."),
        _ => error.to_string(),
    });
}
