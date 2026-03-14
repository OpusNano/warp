use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail};
use russh_sftp::client::{SftpSession, error::Error as SftpError};
use tauri::{AppHandle, Emitter};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

use crate::{
    events::TRANSFER_QUEUE_UPDATED_EVENT,
    models::{QueueDownloadRequest, QueueUploadRequest, TransferConflict, TransferConflictResolution, TransferJob, TransferQueueSnapshot},
    session::SessionManager,
};

const COPY_BUFFER_SIZE: usize = 64 * 1024;

pub struct TransferManager {
    app_handle: AppHandle,
    session_manager: Arc<SessionManager>,
    next_job_id: AtomicU64,
    state: Mutex<TransferState>,
}

struct TransferState {
    jobs: Vec<TransferRecord>,
    active_job_id: Option<String>,
    worker_running: bool,
    cancel_flags: HashMap<String, Arc<AtomicBool>>,
}

#[derive(Clone)]
enum TransferTask {
    Download {
        remote_path: String,
        remote_name: String,
        local_directory: String,
    },
    Upload {
        local_path: String,
        local_name: String,
        remote_directory: String,
    },
}

#[derive(Clone)]
struct TransferRecord {
    id: String,
    task: TransferTask,
    overwrite_approved: bool,
    view: TransferJob,
}

enum TransferRunOutcome {
    Succeeded,
    Failed(String),
    Cancelled,
    AwaitingConflict(TransferConflict),
}

impl TransferManager {
    pub fn new(app_handle: AppHandle, session_manager: Arc<SessionManager>) -> Self {
        Self {
            app_handle,
            session_manager,
            next_job_id: AtomicU64::new(1),
            state: Mutex::new(TransferState {
                jobs: Vec::new(),
                active_job_id: None,
                worker_running: false,
                cancel_flags: HashMap::new(),
            }),
        }
    }

    pub async fn snapshot(&self) -> TransferQueueSnapshot {
        let state = self.state.lock().await;
        snapshot_from_state(&state)
    }

    pub async fn queue_download(self: &Arc<Self>, request: QueueDownloadRequest) -> Result<TransferQueueSnapshot> {
        if request.remote_path.trim().is_empty() || request.remote_name.trim().is_empty() {
            bail!("Select a remote file before queuing a download.")
        }

        if request.local_directory.trim().is_empty() {
            bail!("Choose a local destination before queuing a download.")
        }

        let destination_path = PathBuf::from(&request.local_directory).join(&request.remote_name);
        let record = TransferRecord {
            id: self.next_id(),
            task: TransferTask::Download {
                remote_path: request.remote_path.clone(),
                remote_name: request.remote_name.clone(),
                local_directory: request.local_directory.clone(),
            },
            overwrite_approved: false,
            view: TransferJob {
                id: String::new(),
                protocol: "SFTP".into(),
                direction: "Download".into(),
                name: request.remote_name,
                source_path: request.remote_path,
                destination_path: destination_path.display().to_string(),
                rate: None,
                bytes_total: None,
                bytes_transferred: 0,
                progress_percent: None,
                state: "Queued".into(),
                error_message: None,
                conflict: None,
                can_cancel: true,
            },
        }
        .with_id();

        let snapshot = self.enqueue(record).await;
        self.spawn_worker_if_needed();
        Ok(snapshot)
    }

    pub async fn queue_upload(self: &Arc<Self>, request: QueueUploadRequest) -> Result<TransferQueueSnapshot> {
        if request.local_path.trim().is_empty() || request.local_name.trim().is_empty() {
            bail!("Select a local file before queuing an upload.")
        }

        if request.remote_directory.trim().is_empty() {
            bail!("Connect to a remote directory before queuing an upload.")
        }

        let record = TransferRecord {
            id: self.next_id(),
            task: TransferTask::Upload {
                local_path: request.local_path.clone(),
                local_name: request.local_name.clone(),
                remote_directory: request.remote_directory.clone(),
            },
            overwrite_approved: false,
            view: TransferJob {
                id: String::new(),
                protocol: "SFTP".into(),
                direction: "Upload".into(),
                name: request.local_name.clone(),
                source_path: request.local_path,
                destination_path: join_remote_path(&request.remote_directory, &request.local_name),
                rate: None,
                bytes_total: None,
                bytes_transferred: 0,
                progress_percent: None,
                state: "Queued".into(),
                error_message: None,
                conflict: None,
                can_cancel: true,
            },
        }
        .with_id();

        let snapshot = self.enqueue(record).await;
        self.spawn_worker_if_needed();
        Ok(snapshot)
    }

    pub async fn cancel_transfer(self: &Arc<Self>, job_id: &str) -> TransferQueueSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            let cancel_flag = state.cancel_flags.get(job_id).cloned();
            if let Some(job) = state.jobs.iter_mut().find(|job| job.id == job_id) {
                match job.view.state.as_str() {
                    "Queued" | "Checking" | "AwaitingConflictDecision" => {
                        if let Some(flag) = cancel_flag {
                            flag.store(true, Ordering::Relaxed);
                        }
                        job.view.state = "Cancelled".into();
                        job.view.error_message = None;
                        job.view.conflict = None;
                        job.view.can_cancel = false;
                    }
                    "Running" | "Cancelling" => {
                        job.view.state = "Cancelling".into();
                        job.view.can_cancel = false;
                        if let Some(flag) = cancel_flag {
                            flag.store(true, Ordering::Relaxed);
                        }
                    }
                    _ => {}
                }
            }
            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        snapshot
    }

    pub async fn clear_completed(self: &Arc<Self>) -> TransferQueueSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            state.jobs.retain(|job| !is_terminal_state(&job.view.state));
            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        snapshot
    }

    pub async fn resolve_conflict(
        self: &Arc<Self>,
        job_id: &str,
        resolution: TransferConflictResolution,
    ) -> Result<TransferQueueSnapshot> {
        let action = resolution.action.trim().to_lowercase();
        let snapshot = {
            let mut state = self.state.lock().await;
            let job = state
                .jobs
                .iter_mut()
                .find(|job| job.id == job_id)
                .ok_or_else(|| anyhow!("Transfer job not found."))?;

            if job.view.state != "AwaitingConflictDecision" {
                bail!("This transfer is not waiting on a conflict decision.")
            }

            match action.as_str() {
                "overwrite" => {
                    job.overwrite_approved = true;
                    job.view.state = "Queued".into();
                    job.view.conflict = None;
                    job.view.error_message = None;
                    job.view.can_cancel = true;
                }
                "cancel" => {
                    job.overwrite_approved = false;
                    job.view.state = "Cancelled".into();
                    job.view.conflict = None;
                    job.view.error_message = None;
                    job.view.can_cancel = false;
                }
                _ => bail!("Unknown conflict action: {}", resolution.action),
            }

            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        if action == "overwrite" {
            self.spawn_worker_if_needed();
        }
        Ok(snapshot)
    }

    async fn enqueue(&self, mut record: TransferRecord) -> TransferQueueSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            record.view.id = record.id.clone();
            state.jobs.push(record);
            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        snapshot
    }

    fn spawn_worker_if_needed(self: &Arc<Self>) {
        let manager = self.clone();
        tokio::spawn(async move {
            let should_start = {
                let mut state = manager.state.lock().await;
                if state.worker_running {
                    false
                } else {
                    state.worker_running = true;
                    true
                }
            };

            if should_start {
                manager.run_worker().await;
            }
        });
    }

    async fn run_worker(self: Arc<Self>) {
        loop {
            let next_job = {
                let mut state = self.state.lock().await;
                let Some(index) = state.jobs.iter().position(|job| job.view.state == "Queued") else {
                    state.active_job_id = None;
                    state.worker_running = false;
                    break;
                };

                let job_id = state.jobs[index].id.clone();
                let cancel_flag = Arc::new(AtomicBool::new(false));
                state.active_job_id = Some(job_id.clone());
                state.cancel_flags.insert(job_id.clone(), cancel_flag.clone());
                state.jobs[index].view.state = "Checking".into();
                state.jobs[index].view.error_message = None;
                state.jobs[index].view.conflict = None;
                state.jobs[index].view.can_cancel = true;
                let snapshot = snapshot_from_state(&state);
                let task = state.jobs[index].task.clone();
                let overwrite_approved = state.jobs[index].overwrite_approved;
                let id = state.jobs[index].id.clone();
                (id, task, overwrite_approved, cancel_flag, snapshot)
            };

            self.emit_snapshot(next_job.4);

            let was_cancelled = {
                let state = self.state.lock().await;
                state
                    .jobs
                    .iter()
                    .find(|job| job.id == next_job.0)
                    .is_some_and(|job| job.view.state == "Cancelled")
            };

            if was_cancelled {
                continue;
            }

            let outcome = self
                .run_transfer(next_job.0.clone(), next_job.1, next_job.2, next_job.3.clone())
                .await;

            let snapshot = {
                let mut state = self.state.lock().await;
                state.active_job_id = None;
                state.cancel_flags.remove(&next_job.0);
                let mut session_loss_message = None;
                if let Some(job) = state.jobs.iter_mut().find(|job| job.id == next_job.0) {
                    match outcome {
                        TransferRunOutcome::Succeeded => {
                            job.view.state = "Succeeded".into();
                            job.view.progress_percent = Some(100);
                            job.view.rate = None;
                            job.view.error_message = None;
                            job.view.conflict = None;
                            job.view.can_cancel = false;
                        }
                        TransferRunOutcome::Cancelled => {
                            job.view.state = "Cancelled".into();
                            job.view.rate = None;
                            job.view.error_message = None;
                            job.view.conflict = None;
                            job.view.can_cancel = false;
                        }
                        TransferRunOutcome::Failed(message) => {
                            if is_session_loss_message(&message) {
                                session_loss_message = Some(message.clone());
                            }
                            job.view.state = "Failed".into();
                            job.view.rate = None;
                            job.view.error_message = Some(message);
                            job.view.conflict = None;
                            job.view.can_cancel = false;
                        }
                        TransferRunOutcome::AwaitingConflict(conflict) => {
                            job.view.state = "AwaitingConflictDecision".into();
                            job.view.rate = None;
                            job.view.error_message = None;
                            job.view.conflict = Some(conflict);
                            job.view.can_cancel = true;
                        }
                    }
                }
                (snapshot_from_state(&state), session_loss_message)
            };

            self.emit_snapshot(snapshot.0);
            if let Some(message) = snapshot.1 {
                let _ = self.session_manager.handle_connection_loss(message).await;
            }
        }
    }

    async fn run_transfer(
        &self,
        job_id: String,
        task: TransferTask,
        overwrite_approved: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> TransferRunOutcome {
        match task {
            TransferTask::Download {
                remote_path,
                remote_name,
                local_directory,
            } => match self
                .run_download(
                    &job_id,
                    &remote_path,
                    &remote_name,
                    &local_directory,
                    overwrite_approved,
                    cancel_flag,
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => TransferRunOutcome::Failed(error.to_string()),
            },
            TransferTask::Upload {
                local_path,
                local_name,
                remote_directory,
            } => match self
                .run_upload(
                    &job_id,
                    &local_path,
                    &local_name,
                    &remote_directory,
                    overwrite_approved,
                    cancel_flag,
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => TransferRunOutcome::Failed(error.to_string()),
            },
        }
    }

    async fn run_download(
        &self,
        job_id: &str,
        remote_path: &str,
        remote_name: &str,
        local_directory: &str,
        overwrite_approved: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<TransferRunOutcome> {
        let sftp = self.session_manager.open_transfer_sftp().await?;
        let metadata = sftp
            .metadata(remote_path)
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("inspect the remote file", error)))?;

        if metadata.is_dir() {
            return Ok(TransferRunOutcome::Failed(
                "Warp can only download files in this milestone.".into(),
            ));
        }

        let destination_path = PathBuf::from(local_directory).join(remote_name);
        let temp_path = temp_local_path(&destination_path);

        match inspect_local_conflict(&destination_path) {
            Ok(Some(conflict)) if !overwrite_approved => {
                return Ok(TransferRunOutcome::AwaitingConflict(conflict));
            }
            Ok(Some(conflict)) if !conflict.can_overwrite => {
                return Ok(TransferRunOutcome::Failed(
                    "Warp cannot overwrite a directory destination.".into(),
                ));
            }
            Ok(None) => {}
            Ok(Some(_)) => {}
            Err(error) => return Ok(TransferRunOutcome::Failed(error.to_string())),
        }

        self.mark_running(job_id, metadata.size).await;

        let mut remote_file = sftp
            .open(remote_path)
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("open the remote file", error)))?;

        if temp_path.exists() {
            let _ = fs::remove_file(&temp_path).await;
        }

        let mut local_file = fs::File::create(&temp_path)
            .await
            .with_context(|| format!("failed to create {}", temp_path.display()))?;

        let copy_result = self
            .copy_stream(job_id, metadata.size, cancel_flag, &mut remote_file, &mut local_file)
            .await;

        match copy_result {
            Ok(()) => {}
            Err(CopyFailure::Cancelled) => {
                let _ = fs::remove_file(&temp_path).await;
                return Ok(TransferRunOutcome::Cancelled);
            }
            Err(CopyFailure::Failed(message)) => {
                let _ = fs::remove_file(&temp_path).await;
                return Ok(TransferRunOutcome::Failed(message));
            }
        }

        local_file
            .sync_all()
            .await
            .with_context(|| format!("failed to flush {}", temp_path.display()))?;

        if overwrite_approved && destination_path.exists() {
            fs::remove_file(&destination_path)
                .await
                .with_context(|| format!("failed to replace {}", destination_path.display()))?;
        }

        fs::rename(&temp_path, &destination_path).await.with_context(|| {
            format!(
                "failed to finalize download into {}",
                destination_path.display()
            )
        })?;

        let _ = sftp.close().await;
        Ok(TransferRunOutcome::Succeeded)
    }

    async fn run_upload(
        &self,
        job_id: &str,
        local_path: &str,
        local_name: &str,
        remote_directory: &str,
        overwrite_approved: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<TransferRunOutcome> {
        let local_metadata = fs::metadata(local_path)
            .await
            .with_context(|| format!("failed to inspect {local_path}"))?;

        if local_metadata.is_dir() {
            return Ok(TransferRunOutcome::Failed(
                "Warp can only upload files in this milestone.".into(),
            ));
        }

        let sftp = self.session_manager.open_transfer_sftp().await?;
        let destination_path = join_remote_path(remote_directory, local_name);
        let temp_path = temp_remote_path(remote_directory, local_name, job_id);

        match inspect_remote_conflict(&sftp, &destination_path).await {
            Ok(Some(conflict)) if !overwrite_approved => {
                return Ok(TransferRunOutcome::AwaitingConflict(conflict));
            }
            Ok(Some(conflict)) if !conflict.can_overwrite => {
                return Ok(TransferRunOutcome::Failed(
                    "Warp cannot overwrite a directory destination.".into(),
                ));
            }
            Ok(None) => {}
            Ok(Some(_)) => {}
            Err(error) => return Ok(TransferRunOutcome::Failed(error.to_string())),
        }

        self.mark_running(job_id, Some(local_metadata.len())).await;

        let mut local_file = fs::File::open(local_path)
            .await
            .with_context(|| format!("failed to open {local_path}"))?;
        let mut remote_file = sftp
            .create(&temp_path)
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("create the remote file", error)))?;

        let copy_result = self
            .copy_stream(job_id, Some(local_metadata.len()), cancel_flag, &mut local_file, &mut remote_file)
            .await;

        match copy_result {
            Ok(()) => {}
            Err(CopyFailure::Cancelled) => {
                let _ = remote_file.shutdown().await;
                let _ = sftp.remove_file(&temp_path).await;
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Cancelled);
            }
            Err(CopyFailure::Failed(message)) => {
                let _ = remote_file.shutdown().await;
                let _ = sftp.remove_file(&temp_path).await;
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Failed(message));
            }
        }

        remote_file
            .sync_all()
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("flush the remote file", error)))?;
        remote_file
            .shutdown()
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("close the remote file", SftpError::IO(error.to_string()))))?;

        if overwrite_approved
            && matches!(
            inspect_remote_conflict(&sftp, &destination_path).await,
            Ok(Some(TransferConflict {
                can_overwrite: true,
                ..
            }))
        )
        {
            sftp.remove_file(&destination_path)
                .await
                .map_err(|error| anyhow!(friendly_remote_transfer_error("replace the remote file", error)))?;
        }

        sftp.rename(&temp_path, &destination_path)
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("finalize the upload", error)))?;

        let _ = sftp.close().await;
        Ok(TransferRunOutcome::Succeeded)
    }

    async fn mark_running(&self, job_id: &str, bytes_total: Option<u64>) {
        let snapshot = {
            let mut state = self.state.lock().await;
            if let Some(job) = state.jobs.iter_mut().find(|job| job.id == job_id) {
                if job.view.state == "Cancelled" {
                    job.view.rate = None;
                    job.view.can_cancel = false;
                } else {
                    job.view.state = "Running".into();
                    job.view.bytes_total = bytes_total;
                    job.view.bytes_transferred = 0;
                    job.view.progress_percent = Some(progress_fraction(0, bytes_total));
                    job.view.rate = None;
                }
            }
            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot);
    }

    async fn update_progress(&self, job_id: &str, bytes_transferred: u64, bytes_total: Option<u64>, rate: u64) {
        let snapshot = {
            let mut state = self.state.lock().await;
            if let Some(job) = state.jobs.iter_mut().find(|job| job.id == job_id) {
                job.view.bytes_total = bytes_total;
                job.view.bytes_transferred = bytes_transferred;
                job.view.progress_percent = Some(progress_fraction(bytes_transferred, bytes_total));
                job.view.rate = Some(format_rate(rate));
            }
            snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot);
    }

    async fn copy_stream<R, W>(
        &self,
        job_id: &str,
        bytes_total: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<(), CopyFailure>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
        let started_at = Instant::now();
        let mut last_emit = Instant::now();
        let mut bytes_transferred = 0_u64;

        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(CopyFailure::Cancelled);
            }

            let read = reader.read(&mut buffer).await.map_err(map_copy_io_error)?;
            if read == 0 {
                break;
            }

            writer
                .write_all(&buffer[..read])
                .await
                .map_err(map_copy_io_error)?;

            bytes_transferred += read as u64;
            if last_emit.elapsed().as_millis() >= 125 {
                last_emit = Instant::now();
                let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
                let rate = (bytes_transferred as f64 / elapsed) as u64;
                self.update_progress(job_id, bytes_transferred, bytes_total, rate)
                    .await;
            }
        }

        writer.flush().await.map_err(map_copy_io_error)?;
        let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
        let rate = (bytes_transferred as f64 / elapsed) as u64;
        self.update_progress(job_id, bytes_transferred, bytes_total, rate)
            .await;
        Ok(())
    }

    fn emit_snapshot(&self, snapshot: TransferQueueSnapshot) {
        let _ = self
            .app_handle
            .emit(TRANSFER_QUEUE_UPDATED_EVENT, snapshot);
    }

    fn next_id(&self) -> String {
        format!("tx-{}", self.next_job_id.fetch_add(1, Ordering::Relaxed))
    }
}

impl TransferRecord {
    fn with_id(mut self) -> Self {
        self.view.id = self.id.clone();
        self
    }
}

enum CopyFailure {
    Cancelled,
    Failed(String),
}

fn snapshot_from_state(state: &TransferState) -> TransferQueueSnapshot {
    TransferQueueSnapshot {
        jobs: state.jobs.iter().map(|job| job.view.clone()).collect(),
        active_job_id: state.active_job_id.clone(),
        queued_count: state
            .jobs
            .iter()
            .filter(|job| matches!(job.view.state.as_str(), "Queued" | "Checking" | "AwaitingConflictDecision" | "Running" | "Cancelling"))
            .count(),
        finished_count: state
            .jobs
            .iter()
            .filter(|job| matches!(job.view.state.as_str(), "Succeeded" | "Failed" | "Cancelled"))
            .count(),
    }
}

fn is_terminal_state(state: &str) -> bool {
    matches!(state, "Succeeded" | "Failed" | "Cancelled")
}

fn is_session_loss_message(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("ssh session ended") || message.contains("connection lost after idle")
}

fn inspect_local_conflict(destination_path: &Path) -> Result<Option<TransferConflict>> {
    match std::fs::symlink_metadata(destination_path) {
        Ok(metadata) => Ok(Some(TransferConflict {
            destination_exists: true,
            destination_kind: if metadata.file_type().is_dir() {
                "dir".into()
            } else if metadata.file_type().is_symlink() {
                "symlink".into()
            } else {
                "file".into()
            },
            can_overwrite: !metadata.file_type().is_dir(),
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(anyhow!(
            "Unable to inspect local destination {}: {}",
            destination_path.display(),
            error
        )),
    }
}

async fn inspect_remote_conflict(
    sftp: &SftpSession,
    destination_path: &str,
) -> Result<Option<TransferConflict>> {
    match sftp.symlink_metadata(destination_path).await {
        Ok(metadata) => Ok(Some(TransferConflict {
            destination_exists: true,
            destination_kind: if metadata.is_dir() {
                "dir".into()
            } else if metadata.is_symlink() {
                "symlink".into()
            } else {
                "file".into()
            },
            can_overwrite: !metadata.is_dir(),
        })),
        Err(SftpError::Status(status))
            if matches!(status.status_code, russh_sftp::protocol::StatusCode::NoSuchFile) =>
        {
            Ok(None)
        }
        Err(error) => Err(anyhow!(friendly_remote_transfer_error(
            "inspect the remote destination",
            error
        ))),
    }
}

fn temp_local_path(destination_path: &Path) -> PathBuf {
    let name = destination_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("transfer");
    destination_path.with_file_name(format!(".{name}.warp-part"))
}

fn temp_remote_path(remote_directory: &str, local_name: &str, job_id: &str) -> String {
    join_remote_path(remote_directory, &format!(".{local_name}.warp-part-{job_id}"))
}

fn join_remote_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

fn progress_fraction(bytes_transferred: u64, bytes_total: Option<u64>) -> u8 {
    match bytes_total {
        Some(0) => 100,
        Some(total) => ((bytes_transferred.saturating_mul(100)).min(total.saturating_mul(100)) / total) as u8,
        None => 0,
    }
}

fn format_rate(bytes_per_second: u64) -> String {
    if bytes_per_second < 1024 {
        return format!("{} B/s", bytes_per_second);
    }

    let units = ["KB/s", "MB/s", "GB/s", "TB/s"];
    let mut value = bytes_per_second as f64 / 1024.0;
    let mut unit = units[0];
    for next in units.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }

    if value < 10.0 {
        format!("{value:.1} {unit}")
    } else {
        format!("{value:.0} {unit}")
    }
}

fn map_copy_io_error(error: std::io::Error) -> CopyFailure {
    let message = error.to_string().to_lowercase();
    if message.contains("broken pipe")
        || message.contains("connection reset")
        || message.contains("unexpected eof")
        || message.contains("channel closed")
        || message.contains("session closed")
    {
        return CopyFailure::Failed(
            "The SSH session ended during the transfer. Reconnect and try again.".into(),
        );
    }

    CopyFailure::Failed(error.to_string())
}

fn friendly_remote_transfer_error(action: &str, error: SftpError) -> String {
    let message = error.to_string().to_lowercase();
    if message.contains("permission denied") {
        return format!("Permission denied while trying to {action}.");
    }

    if message.contains("no such file") {
        return format!("The remote path is no longer available, so Warp could not {action}.");
    }

    if message.contains("disconnect")
        || message.contains("broken pipe")
        || message.contains("connection reset")
        || message.contains("unexpected eof")
        || message.contains("channel closed")
        || message.contains("session closed")
    {
        return format!("The SSH session ended while trying to {action}.");
    }

    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{format_rate, join_remote_path};

    #[test]
    fn joins_remote_paths_without_double_slashes() {
        assert_eq!(join_remote_path("/", "file.txt"), "/file.txt");
        assert_eq!(join_remote_path("/srv/www", "file.txt"), "/srv/www/file.txt");
    }

    #[test]
    fn formats_transfer_rates() {
        assert_eq!(format_rate(900), "900 B/s");
        assert_eq!(format_rate(2048), "2.0 KB/s");
    }
}
