use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail};
use russh_sftp::{
    client::{SftpSession, error::Error as SftpError, fs::DirEntry as SftpDirEntry},
    protocol::FileType as SftpFileType,
};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};

use crate::{
    events::TRANSFER_QUEUE_UPDATED_EVENT,
    models::{
        QueueDownloadRequest, QueueUploadRequest, TransferConflict, TransferConflictResolution,
        TransferJob, TransferJobSummary, TransferQueueSnapshot, TransferSelectionItem,
    },
    remote_sftp::RemoteSftpEngine,
    session::SessionManager,
};

const COPY_BUFFER_SIZE: usize = 64 * 1024;

pub struct TransferManager<R: Runtime = tauri::Wry> {
    app_handle: AppHandle<R>,
    session_manager: Arc<SessionManager<R>>,
    next_job_id: AtomicU64,
    next_snapshot_sequence: AtomicU64,
    state: Mutex<TransferState>,
}

struct TransferState {
    batches: Vec<TransferBatchRecord>,
    active_job_id: Option<String>,
    worker_running: bool,
    cancel_flags: HashMap<String, Arc<AtomicBool>>,
}

struct TransferBatchRecord {
    id: String,
    direction: TransferDirection,
    destination_root: String,
    conflict_policy: ConflictPolicy,
    pause_message: Option<String>,
    cancel_requested: bool,
    view: TransferJob,
    children: Vec<TransferChildRecord>,
}

struct TransferChildRecord {
    id: String,
    batch_id: String,
    item_kind: PlannedItemKind,
    task: TransferChildTask,
    overwrite_approved: bool,
    view: TransferJob,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TransferDirection {
    Upload,
    Download,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConflictPolicy {
    Ask,
    OverwriteAll,
    SkipAll,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlannedItemKind {
    File,
    Directory,
}

#[derive(Clone)]
enum TransferChildTask {
    CreateRemoteDir { remote_path: String },
    CreateLocalDir { local_path: String },
    UploadFile { local_path: String, remote_path: String },
    DownloadFile { remote_path: String, local_path: String },
}

struct PlannedBatch {
    direction: TransferDirection,
    destination_root: String,
    display_name: String,
    source_label: String,
    children: Vec<PlannedChild>,
    summary: TransferJobSummary,
}

struct PlannedChild {
    item_kind: PlannedItemKind,
    display_name: String,
    source_path: String,
    destination_path: String,
    bytes_total: Option<u64>,
    task: TransferChildTask,
}

enum TransferRunOutcome {
    Succeeded,
    Failed(String),
    Cancelled,
    AwaitingConflict(TransferConflict),
    Skipped,
}

enum CopyFailure {
    Cancelled,
    Failed(String),
}

impl<R: Runtime> TransferManager<R> {
    pub fn new(app_handle: AppHandle<R>, session_manager: Arc<SessionManager<R>>) -> Self {
        Self {
            app_handle,
            session_manager,
            next_job_id: AtomicU64::new(1),
            next_snapshot_sequence: AtomicU64::new(1),
            state: Mutex::new(TransferState {
                batches: Vec::new(),
                active_job_id: None,
                worker_running: false,
                cancel_flags: HashMap::new(),
            }),
        }
    }

    pub async fn snapshot(&self) -> TransferQueueSnapshot {
        let state = self.state.lock().await;
        self.snapshot_from_state(&state)
    }

    pub async fn queue_download(self: &Arc<Self>, request: QueueDownloadRequest) -> Result<TransferQueueSnapshot> {
        if request.entries.is_empty() {
            bail!("Select one or more remote entries before queuing a download.")
        }

        if request.local_directory.trim().is_empty() {
            bail!("Choose a local destination before queuing a download.")
        }

        let planned = plan_download_batch(&self.session_manager, request).await?;
        let snapshot = self.enqueue_batch(planned).await;
        self.spawn_worker_if_needed();
        Ok(snapshot)
    }

    pub async fn queue_upload(self: &Arc<Self>, request: QueueUploadRequest) -> Result<TransferQueueSnapshot> {
        if request.entries.is_empty() {
            bail!("Select one or more local entries before queuing an upload.")
        }

        if request.remote_directory.trim().is_empty() {
            bail!("Connect to a remote directory before queuing an upload.")
        }

        let planned = plan_upload_batch(request).await?;
        let snapshot = self.enqueue_batch(planned).await;
        self.spawn_worker_if_needed();
        Ok(snapshot)
    }

    pub async fn cancel_transfer(self: &Arc<Self>, job_id: &str) -> TransferQueueSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            if cancel_batch_by_id(&mut state, job_id) {
                self.snapshot_from_state(&state)
            } else {
                cancel_child_by_id(&mut state, job_id);
                self.snapshot_from_state(&state)
            }
        };
        self.emit_snapshot(snapshot.clone());
        self.spawn_worker_if_needed();
        snapshot
    }

    pub async fn retry_transfer(self: &Arc<Self>, job_id: &str) -> TransferQueueSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            if !retry_batch_by_id(&mut state, job_id) {
                retry_child_by_id(&mut state, job_id);
            }
            self.snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        self.spawn_worker_if_needed();
        snapshot
    }

    pub async fn clear_completed(self: &Arc<Self>) -> TransferQueueSnapshot {
        let snapshot = {
            let mut state = self.state.lock().await;
            state.batches.retain(|batch| !is_terminal_state(&batch.view.state));
            self.snapshot_from_state(&state)
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
            let (batch_index, child_index) = find_conflict_target_indices(&state, job_id)
                .ok_or_else(|| anyhow!("Transfer job not found."))?;
            let batch = &mut state.batches[batch_index];
            let child = &mut batch.children[child_index];

            if child.view.state != "AwaitingConflictDecision" {
                bail!("This transfer is not waiting on a conflict decision.")
            }

            match action.as_str() {
                "overwrite" => {
                    child.overwrite_approved = true;
                    child.view.state = "Queued".into();
                    child.view.conflict = None;
                    child.view.error_message = None;
                    child.view.can_cancel = true;
                }
                "skip" => {
                    child.view.state = "Skipped".into();
                    child.view.conflict = None;
                    child.view.error_message = None;
                    child.view.can_cancel = false;
                }
                "overwriteall" => {
                    batch.conflict_policy = ConflictPolicy::OverwriteAll;
                    child.overwrite_approved = true;
                    child.view.state = "Queued".into();
                    child.view.conflict = None;
                    child.view.error_message = None;
                    child.view.can_cancel = true;
                }
                "skipall" => {
                    batch.conflict_policy = ConflictPolicy::SkipAll;
                    child.view.state = "Skipped".into();
                    child.view.conflict = None;
                    child.view.error_message = None;
                    child.view.can_cancel = false;
                }
                "cancelbatch" => {
                    batch.cancel_requested = true;
                    child.view.state = "Cancelled".into();
                    child.view.conflict = None;
                    child.view.error_message = None;
                    child.view.can_cancel = false;
                }
                _ => bail!("Unknown conflict action: {}", resolution.action),
            }

            refresh_batch_aggregate(batch);
            self.snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot.clone());
        self.spawn_worker_if_needed();
        Ok(snapshot)
    }

    async fn enqueue_batch(&self, planned: PlannedBatch) -> TransferQueueSnapshot {
        let batch_id = self.next_id();
        let summary = planned.summary.clone();
        let view = TransferJob {
            id: batch_id.clone(),
            kind: "batch".into(),
            batch_id: Some(batch_id.clone()),
            parent_id: None,
            protocol: "SFTP".into(),
            direction: planned.direction.label().into(),
            name: planned.display_name,
            source_path: planned.source_label,
            destination_path: planned.destination_root.clone(),
            rate: None,
            bytes_total: None,
            bytes_transferred: 0,
            progress_percent: Some(0),
            state: "Queued".into(),
            error_message: None,
            conflict: None,
            can_cancel: true,
            can_retry: false,
            summary: Some(summary),
            current_item_label: None,
        };

        let children = planned
            .children
            .into_iter()
            .map(|child| {
                let id = self.next_id();
                let direction = planned.direction.label().to_string();
                TransferChildRecord {
                    id: id.clone(),
                    batch_id: batch_id.clone(),
                    item_kind: child.item_kind,
                    task: child.task,
                    overwrite_approved: false,
                    view: TransferJob {
                        id,
                        kind: "child".into(),
                        batch_id: Some(batch_id.clone()),
                        parent_id: Some(batch_id.clone()),
                        protocol: "SFTP".into(),
                        direction,
                        name: child.display_name,
                        source_path: child.source_path,
                        destination_path: child.destination_path,
                        rate: None,
                        bytes_total: child.bytes_total,
                        bytes_transferred: 0,
                        progress_percent: Some(if child.item_kind == PlannedItemKind::Directory { 0 } else { progress_fraction(0, child.bytes_total) }),
                        state: "Queued".into(),
                        error_message: None,
                        conflict: None,
                        can_cancel: true,
                        can_retry: false,
                        summary: None,
                        current_item_label: None,
                    },
                }
            })
            .collect::<Vec<_>>();

        let snapshot = {
            let mut state = self.state.lock().await;
            let mut batch = TransferBatchRecord {
                id: batch_id,
                direction: planned.direction,
                destination_root: planned.destination_root,
                conflict_policy: ConflictPolicy::Ask,
                pause_message: None,
                cancel_requested: false,
                view,
                children,
            };
            refresh_batch_aggregate(&mut batch);
            state.batches.push(batch);
            self.snapshot_from_state(&state)
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
                finalize_cancelled_batches(&mut state);

                let Some((batch_index, child_index)) = next_runnable_child(&state) else {
                    state.active_job_id = None;
                    state.worker_running = false;
                    break;
                };

                let job_id = state.batches[batch_index].children[child_index].id.clone();
                let cancel_flag = Arc::new(AtomicBool::new(false));
                state.active_job_id = Some(job_id.clone());
                state.cancel_flags.insert(job_id.clone(), cancel_flag.clone());
                {
                    let batch = &mut state.batches[batch_index];
                    batch.pause_message = None;
                    let child = &mut batch.children[child_index];
                    child.view.state = "Checking".into();
                    child.view.error_message = None;
                    child.view.conflict = None;
                    child.view.can_cancel = true;
                    child.view.can_retry = false;
                    batch.view.current_item_label = Some(child.view.name.clone());
                    refresh_batch_aggregate(batch);
                }
                let snapshot = self.snapshot_from_state(&state);

                (
                    job_id,
                    state.batches[batch_index].id.clone(),
                    state.batches[batch_index].children[child_index].task.clone(),
                    state.batches[batch_index].children[child_index].overwrite_approved
                        || state.batches[batch_index].conflict_policy == ConflictPolicy::OverwriteAll,
                    state.batches[batch_index].conflict_policy == ConflictPolicy::SkipAll,
                    cancel_flag,
                    snapshot,
                )
            };

            self.emit_snapshot(next_job.6);

            let outcome = self
                .run_child_transfer(next_job.0.clone(), next_job.2, next_job.3, next_job.4, next_job.5.clone())
                .await;

            let snapshot = {
                let mut state = self.state.lock().await;
                state.active_job_id = None;
                state.cancel_flags.remove(&next_job.0);
                let mut session_loss_message = None;
                if let Some(batch) = state.batches.iter_mut().find(|batch| batch.id == next_job.1) {
                    if let Some(child) = batch.children.iter_mut().find(|child| child.id == next_job.0) {
                        match outcome {
                            TransferRunOutcome::Succeeded => {
                                child.view.state = "Succeeded".into();
                                child.view.progress_percent = Some(100);
                                child.view.rate = None;
                                child.view.error_message = None;
                                child.view.conflict = None;
                                child.view.can_cancel = false;
                                child.view.can_retry = false;
                            }
                            TransferRunOutcome::Cancelled => {
                                child.view.state = "Cancelled".into();
                                child.view.rate = None;
                                child.view.error_message = None;
                                child.view.conflict = None;
                                child.view.can_cancel = false;
                                child.view.can_retry = true;
                            }
                            TransferRunOutcome::Skipped => {
                                child.view.state = "Skipped".into();
                                child.view.rate = None;
                                child.view.error_message = None;
                                child.view.conflict = None;
                                child.view.can_cancel = false;
                                child.view.can_retry = true;
                            }
                            TransferRunOutcome::Failed(message) => {
                                if is_session_loss_message(&message) {
                                    session_loss_message = Some(message.clone());
                                    batch.pause_message = Some(message.clone());
                                }
                                child.view.state = "Failed".into();
                                child.view.rate = None;
                                child.view.error_message = Some(message);
                                child.view.conflict = None;
                                child.view.can_cancel = false;
                                child.view.can_retry = true;
                            }
                            TransferRunOutcome::AwaitingConflict(conflict) => {
                                child.view.state = "AwaitingConflictDecision".into();
                                child.view.rate = None;
                                child.view.error_message = None;
                                child.view.conflict = Some(conflict);
                                child.view.can_cancel = true;
                                child.view.can_retry = false;
                            }
                        }
                    }

                    if batch.cancel_requested {
                        mark_remaining_children_cancelled(batch);
                    }
                    refresh_batch_aggregate(batch);
                }

                if let Some(message) = session_loss_message.as_ref() {
                    pause_batches_for_disconnect(&mut state, message);
                }

                (self.snapshot_from_state(&state), session_loss_message)
            };

            self.emit_snapshot(snapshot.0);
            if let Some(message) = snapshot.1 {
                let _ = self.session_manager.handle_connection_loss(message).await;
            }
        }
    }

    async fn run_child_transfer(
        &self,
        job_id: String,
        task: TransferChildTask,
        overwrite_approved: bool,
        skip_conflicts: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> TransferRunOutcome {
        match task {
            TransferChildTask::CreateRemoteDir { remote_path } => {
                match self.run_create_remote_dir(&job_id, &remote_path, cancel_flag).await {
                    Ok(outcome) => outcome,
                    Err(error) => TransferRunOutcome::Failed(error.to_string()),
                }
            }
            TransferChildTask::CreateLocalDir { local_path } => {
                match self.run_create_local_dir(&job_id, &local_path, cancel_flag).await {
                    Ok(outcome) => outcome,
                    Err(error) => TransferRunOutcome::Failed(error.to_string()),
                }
            }
            TransferChildTask::UploadFile {
                local_path,
                remote_path,
            } => match self
                .run_upload_file(&job_id, &local_path, &remote_path, overwrite_approved, skip_conflicts, cancel_flag)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => TransferRunOutcome::Failed(error.to_string()),
            },
            TransferChildTask::DownloadFile {
                remote_path,
                local_path,
            } => match self
                .run_download_file(&job_id, &remote_path, &local_path, overwrite_approved, skip_conflicts, cancel_flag)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => TransferRunOutcome::Failed(error.to_string()),
            },
        }
    }

    async fn run_create_remote_dir(
        &self,
        job_id: &str,
        remote_path: &str,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<TransferRunOutcome> {
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(TransferRunOutcome::Cancelled);
        }

        self.mark_running(job_id, Some(0)).await;
        let sftp = self.session_manager.open_transfer_sftp().await?;
        match inspect_remote_conflict(&sftp, remote_path).await {
            Ok(Some(conflict)) if conflict.destination_kind == "dir" => {
                let _ = sftp.close().await;
                Ok(TransferRunOutcome::Succeeded)
            }
            Ok(Some(_)) => {
                let _ = sftp.close().await;
                Ok(TransferRunOutcome::Failed(
                    "Warp cannot replace a file destination with a directory.".into(),
                ))
            }
            Ok(None) => {
                let parent = parent_remote_path(remote_path);
                ensure_remote_directory_chain(&sftp, &parent).await?;
                sftp.create_dir(remote_path)
                    .await
                    .map_err(|error| anyhow!(classify_transfer_remote_error("create_dir", remote_path, error)))?;
                let _ = sftp.close().await;
                self.update_progress(job_id, 0, Some(0), 0).await;
                Ok(TransferRunOutcome::Succeeded)
            }
            Err(error) => {
                let _ = sftp.close().await;
                Ok(TransferRunOutcome::Failed(error.to_string()))
            }
        }
    }

    async fn run_create_local_dir(
        &self,
        job_id: &str,
        local_path: &str,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<TransferRunOutcome> {
        if cancel_flag.load(Ordering::Relaxed) {
            return Ok(TransferRunOutcome::Cancelled);
        }

        self.mark_running(job_id, Some(0)).await;
        let path = PathBuf::from(local_path);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_dir() {
                    self.update_progress(job_id, 0, Some(0), 0).await;
                    return Ok(TransferRunOutcome::Succeeded);
                }

                return Ok(TransferRunOutcome::Failed(
                    "Warp cannot replace a file destination with a directory.".into(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(anyhow!("Unable to inspect local destination {}: {}", path.display(), error)),
        }

        fs::create_dir_all(&path)
            .await
            .with_context(|| format!("failed to create directory {}", path.display()))?;
        self.update_progress(job_id, 0, Some(0), 0).await;
        Ok(TransferRunOutcome::Succeeded)
    }

    async fn run_download_file(
        &self,
        job_id: &str,
        remote_path: &str,
        local_path: &str,
        overwrite_approved: bool,
        skip_conflicts: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<TransferRunOutcome> {
        let sftp = self.session_manager.open_transfer_sftp().await?;
        let metadata = sftp
            .metadata(remote_path)
            .await
            .map_err(|error| anyhow!(classify_transfer_remote_error("inspect_file", remote_path, error)))?;

        if metadata.is_dir() {
            let _ = sftp.close().await;
            return Ok(TransferRunOutcome::Failed(
                "Warp does not recurse through symlinked directories during download.".into(),
            ));
        }

        let destination_path = PathBuf::from(local_path);
        let temp_path = temp_local_path(&destination_path, job_id);
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to prepare {}", parent.display()))?;
        }

        match inspect_local_conflict(&destination_path) {
            Ok(Some(_conflict)) if skip_conflicts && !overwrite_approved => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Skipped);
            }
            Ok(Some(conflict)) if !overwrite_approved => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::AwaitingConflict(contextualize_conflict(
                    conflict,
                    local_path,
                    remote_path,
                    "file",
                )));
            }
            Ok(Some(conflict)) if !conflict.can_overwrite => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Failed(
                    "Warp cannot overwrite a directory destination.".into(),
                ));
            }
            Ok(None) | Ok(Some(_)) => {}
            Err(error) => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Failed(error.to_string()));
            }
        }

        self.mark_running(job_id, metadata.size).await;

        let mut remote_file = sftp
            .open(remote_path)
            .await
            .map_err(|error| anyhow!(classify_transfer_remote_error("open_file", remote_path, error)))?;

        if temp_path.exists() {
            let _ = fs::remove_file(&temp_path).await;
        }

        let mut local_file = fs::File::create(&temp_path)
            .await
            .with_context(|| format!("failed to create {}", temp_path.display()))?;

        match self
            .copy_stream(job_id, metadata.size, cancel_flag, &mut remote_file, &mut local_file)
            .await
        {
            Ok(()) => {}
            Err(CopyFailure::Cancelled) => {
                let _ = fs::remove_file(&temp_path).await;
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Cancelled);
            }
            Err(CopyFailure::Failed(message)) => {
                let _ = fs::remove_file(&temp_path).await;
                let _ = sftp.close().await;
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

    async fn run_upload_file(
        &self,
        job_id: &str,
        local_path: &str,
        remote_path: &str,
        overwrite_approved: bool,
        skip_conflicts: bool,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<TransferRunOutcome> {
        let local_metadata = fs::metadata(local_path)
            .await
            .with_context(|| format!("failed to inspect {local_path}"))?;

        if local_metadata.is_dir() {
            return Ok(TransferRunOutcome::Failed(
                "Warp does not recurse through symlinked directories during upload.".into(),
            ));
        }

        let sftp = self.session_manager.open_transfer_sftp().await?;
        let remote_parent = parent_remote_path(remote_path);
        let remote_name = Path::new(remote_path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("transfer");
        let temp_path = temp_remote_path(&remote_parent, remote_name, job_id);

        ensure_remote_directory_chain(&sftp, &remote_parent).await?;

        match inspect_remote_conflict(&sftp, remote_path).await {
            Ok(Some(_conflict)) if skip_conflicts && !overwrite_approved => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Skipped);
            }
            Ok(Some(conflict)) if !overwrite_approved => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::AwaitingConflict(contextualize_conflict(
                    conflict,
                    local_path,
                    remote_path,
                    "file",
                )));
            }
            Ok(Some(conflict)) if !conflict.can_overwrite => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Failed(
                    "Warp cannot overwrite a directory destination.".into(),
                ));
            }
            Ok(None) | Ok(Some(_)) => {}
            Err(error) => {
                let _ = sftp.close().await;
                return Ok(TransferRunOutcome::Failed(error.to_string()));
            }
        }

        self.mark_running(job_id, Some(local_metadata.len())).await;

        let mut local_file = fs::File::open(local_path)
            .await
            .with_context(|| format!("failed to open {local_path}"))?;
        let mut remote_file = sftp
            .create(&temp_path)
            .await
            .map_err(|error| anyhow!(classify_transfer_remote_error("create_file", &temp_path, error)))?;

        match self
            .copy_stream(job_id, Some(local_metadata.len()), cancel_flag, &mut local_file, &mut remote_file)
            .await
        {
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
            .map_err(|error| anyhow!(classify_transfer_remote_error("flush_file", remote_path, error)))?;
        remote_file.shutdown().await.map_err(|error| {
            anyhow!(classify_transfer_remote_error(
                "close_file",
                remote_path,
                SftpError::IO(error.to_string())
            ))
        })?;

        if overwrite_approved
            && matches!(
                inspect_remote_conflict(&sftp, remote_path).await,
                Ok(Some(TransferConflict {
                    can_overwrite: true,
                    ..
                }))
            )
        {
            sftp.remove_file(remote_path)
                .await
                .map_err(|error| anyhow!(classify_transfer_remote_error("replace_file", remote_path, error)))?;
        }

        sftp.rename(&temp_path, remote_path)
            .await
            .map_err(|error| anyhow!(classify_transfer_remote_error("finalize_upload", remote_path, error)))?;

        let _ = sftp.close().await;
        Ok(TransferRunOutcome::Succeeded)
    }

    async fn mark_running(&self, job_id: &str, bytes_total: Option<u64>) {
        let snapshot = {
            let mut state = self.state.lock().await;
            if let Some((batch_index, child_index)) = find_child_indices(&state, job_id) {
                let batch = &mut state.batches[batch_index];
                let child = &mut batch.children[child_index];
                if child.view.state == "Cancelled" {
                    child.view.rate = None;
                    child.view.can_cancel = false;
                } else {
                    child.view.state = "Running".into();
                    child.view.bytes_total = bytes_total;
                    child.view.bytes_transferred = 0;
                    child.view.progress_percent = Some(progress_fraction(0, bytes_total));
                    child.view.rate = None;
                    child.view.can_cancel = true;
                    child.view.can_retry = false;
                    batch.view.current_item_label = Some(child.view.name.clone());
                    refresh_batch_aggregate(batch);
                }
            }
            self.snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot);
    }

    async fn update_progress(&self, job_id: &str, bytes_transferred: u64, bytes_total: Option<u64>, rate: u64) {
        let snapshot = {
            let mut state = self.state.lock().await;
            if let Some((batch_index, child_index)) = find_child_indices(&state, job_id) {
                let batch = &mut state.batches[batch_index];
                let child = &mut batch.children[child_index];
                child.view.bytes_total = bytes_total;
                child.view.bytes_transferred = bytes_transferred;
                child.view.progress_percent = Some(progress_fraction(bytes_transferred, bytes_total));
                child.view.rate = Some(format_rate(rate));
                batch.view.current_item_label = Some(child.view.name.clone());
                refresh_batch_aggregate(batch);
            }
            self.snapshot_from_state(&state)
        };
        self.emit_snapshot(snapshot);
    }

    async fn copy_stream<RR, WW>(
        &self,
        job_id: &str,
        bytes_total: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
        reader: &mut RR,
        writer: &mut WW,
    ) -> Result<(), CopyFailure>
    where
        RR: tokio::io::AsyncRead + Unpin,
        WW: tokio::io::AsyncWrite + Unpin,
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

    fn snapshot_from_state(&self, state: &TransferState) -> TransferQueueSnapshot {
        let mut snapshot = snapshot_from_state(state);
        snapshot.sequence = self.next_snapshot_sequence.fetch_add(1, Ordering::Relaxed);
        snapshot
    }

    #[cfg(test)]
    pub(crate) async fn debug_all_jobs(&self) -> Vec<TransferJob> {
        let state = self.state.lock().await;
        let mut jobs = Vec::new();
        for batch in &state.batches {
            jobs.push(batch.view.clone());
            jobs.extend(batch.children.iter().map(|child| child.view.clone()));
        }
        jobs
    }
}

async fn plan_upload_batch(request: QueueUploadRequest) -> Result<PlannedBatch> {
    let mut children = Vec::new();
    let mut total_files = 0_usize;
    let mut total_directories = 0_usize;

    for entry in &request.entries {
        let destination_path = join_remote_path(&request.remote_directory, &entry.name);
        match entry.kind.as_str() {
            "dir" => {
                total_directories += 1;
                children.push(PlannedChild {
                    item_kind: PlannedItemKind::Directory,
                    display_name: entry.name.clone(),
                    source_path: entry.path.clone(),
                    destination_path: destination_path.clone(),
                    bytes_total: Some(0),
                    task: TransferChildTask::CreateRemoteDir {
                        remote_path: destination_path.clone(),
                    },
                });
                walk_local_directory_upload(&entry.path, &destination_path, &mut children, &mut total_files, &mut total_directories)
                    .await?;
            }
            _ => {
                total_files += 1;
                children.push(PlannedChild {
                    item_kind: PlannedItemKind::File,
                    display_name: entry.name.clone(),
                    source_path: entry.path.clone(),
                    destination_path: destination_path.clone(),
                    bytes_total: local_planned_size(Path::new(&entry.path)),
                    task: TransferChildTask::UploadFile {
                        local_path: entry.path.clone(),
                        remote_path: destination_path,
                    },
                });
            }
        }
    }

    Ok(PlannedBatch {
        direction: TransferDirection::Upload,
        destination_root: request.remote_directory,
        display_name: batch_display_name(&request.entries),
        source_label: batch_source_label(&request.entries),
        children,
        summary: TransferJobSummary {
            total_files,
            total_directories,
            completed_files: 0,
            failed_files: 0,
            skipped_files: 0,
        },
    })
}

async fn plan_download_batch<R: Runtime>(
    session_manager: &Arc<SessionManager<R>>,
    request: QueueDownloadRequest,
) -> Result<PlannedBatch>
{
    let sftp = session_manager.open_transfer_sftp().await?;
    let mut children = Vec::new();
    let mut total_files = 0_usize;
    let mut total_directories = 0_usize;

    for entry in &request.entries {
        let destination_path = PathBuf::from(&request.local_directory).join(&entry.name);
        match entry.kind.as_str() {
            "dir" => {
                total_directories += 1;
                children.push(PlannedChild {
                    item_kind: PlannedItemKind::Directory,
                    display_name: entry.name.clone(),
                    source_path: entry.path.clone(),
                    destination_path: destination_path.display().to_string(),
                    bytes_total: Some(0),
                    task: TransferChildTask::CreateLocalDir {
                        local_path: destination_path.display().to_string(),
                    },
                });
                walk_remote_directory_download(&sftp, &entry.path, &destination_path, &mut children, &mut total_files, &mut total_directories)
                    .await?;
            }
            _ => {
                total_files += 1;
                children.push(PlannedChild {
                    item_kind: PlannedItemKind::File,
                    display_name: entry.name.clone(),
                    source_path: entry.path.clone(),
                    destination_path: destination_path.display().to_string(),
                    bytes_total: None,
                    task: TransferChildTask::DownloadFile {
                        remote_path: entry.path.clone(),
                        local_path: destination_path.display().to_string(),
                    },
                });
            }
        }
    }

    let _ = sftp.close().await;

    Ok(PlannedBatch {
        direction: TransferDirection::Download,
        destination_root: request.local_directory,
        display_name: batch_display_name(&request.entries),
        source_label: batch_source_label(&request.entries),
        children,
        summary: TransferJobSummary {
            total_files,
            total_directories,
            completed_files: 0,
            failed_files: 0,
            skipped_files: 0,
        },
    })
}

async fn walk_local_directory_upload(
    root_local_path: &str,
    root_remote_path: &str,
    children: &mut Vec<PlannedChild>,
    total_files: &mut usize,
    total_directories: &mut usize,
) -> Result<()> {
    let mut stack = vec![(PathBuf::from(root_local_path), root_remote_path.to_string())];

    while let Some((local_dir, remote_dir)) = stack.pop() {
        let entries = read_local_dir_entries(&local_dir).await?;
        for (name, file_type) in entries.into_iter().rev() {
            let child_local_path = local_dir.join(&name);
            let child_remote_path = join_remote_path(&remote_dir, &name.to_string_lossy());
            if file_type.is_dir() {
                *total_directories += 1;
                children.push(PlannedChild {
                    item_kind: PlannedItemKind::Directory,
                    display_name: name.to_string_lossy().into_owned(),
                    source_path: child_local_path.display().to_string(),
                    destination_path: child_remote_path.clone(),
                    bytes_total: Some(0),
                    task: TransferChildTask::CreateRemoteDir {
                        remote_path: child_remote_path.clone(),
                    },
                });
                stack.push((child_local_path, child_remote_path));
            } else {
                *total_files += 1;
                children.push(PlannedChild {
                    item_kind: PlannedItemKind::File,
                    display_name: name.to_string_lossy().into_owned(),
                    source_path: child_local_path.display().to_string(),
                    destination_path: child_remote_path.clone(),
                    bytes_total: local_planned_size(&child_local_path),
                    task: TransferChildTask::UploadFile {
                        local_path: child_local_path.display().to_string(),
                        remote_path: child_remote_path,
                    },
                });
            }
        }
    }

    Ok(())
}

async fn walk_remote_directory_download(
    sftp: &SftpSession,
    root_remote_path: &str,
    root_local_path: &Path,
    children: &mut Vec<PlannedChild>,
    total_files: &mut usize,
    total_directories: &mut usize,
) -> Result<()> {
    let mut stack = vec![(root_remote_path.to_string(), root_local_path.to_path_buf())];

    while let Some((remote_dir, local_dir)) = stack.pop() {
        let mut entries = sftp
            .read_dir(&remote_dir)
            .await
            .map_err(|error| anyhow!(friendly_remote_transfer_error("list the remote directory", error)))?
            .collect::<Vec<SftpDirEntry>>();
        entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

        for entry in entries.into_iter().rev() {
            let name = entry.file_name();
            let child_remote_path = RemoteSftpEngine::child_path(&remote_dir, &name)?;
            let child_local_path = local_dir.join(&name);
            match entry.file_type() {
                SftpFileType::Dir => {
                    *total_directories += 1;
                    children.push(PlannedChild {
                        item_kind: PlannedItemKind::Directory,
                        display_name: name.clone(),
                        source_path: child_remote_path.clone(),
                        destination_path: child_local_path.display().to_string(),
                        bytes_total: Some(0),
                        task: TransferChildTask::CreateLocalDir {
                            local_path: child_local_path.display().to_string(),
                        },
                    });
                    stack.push((child_remote_path, child_local_path));
                }
                _ => {
                    *total_files += 1;
                    children.push(PlannedChild {
                        item_kind: PlannedItemKind::File,
                        display_name: name,
                        source_path: child_remote_path.clone(),
                        destination_path: child_local_path.display().to_string(),
                        bytes_total: entry.metadata().size,
                        task: TransferChildTask::DownloadFile {
                            remote_path: child_remote_path,
                            local_path: child_local_path.display().to_string(),
                        },
                    });
                }
            }
        }
    }

    Ok(())
}

async fn read_local_dir_entries(path: &Path) -> Result<Vec<(OsString, std::fs::FileType)>> {
    let mut read_dir = fs::read_dir(path)
        .await
        .with_context(|| format!("failed to read directory {}", path.display()))?;
    let mut entries = Vec::new();
    while let Some(entry) = read_dir.next_entry().await? {
        entries.push((entry.file_name(), entry.file_type().await?));
    }
    entries.sort_by(|left, right| left.0.to_string_lossy().cmp(&right.0.to_string_lossy()));
    Ok(entries)
}

fn local_planned_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().and_then(|metadata| if metadata.is_file() { Some(metadata.len()) } else { None })
}

fn batch_display_name(entries: &[TransferSelectionItem]) -> String {
    match entries {
        [entry] => entry.name.clone(),
        _ => format!("{} items", entries.len()),
    }
}

fn batch_source_label(entries: &[TransferSelectionItem]) -> String {
    match entries {
        [entry] => entry.path.clone(),
        _ => format!("{} selected entries", entries.len()),
    }
}

fn snapshot_from_state(state: &TransferState) -> TransferQueueSnapshot {
    let mut jobs = Vec::new();
    for batch in &state.batches {
        jobs.push(batch.view.clone());
    }

    TransferQueueSnapshot {
        sequence: 0,
        jobs,
        active_job_id: state.active_job_id.clone(),
        queued_count: state
            .batches
            .iter()
            .map(|batch| {
                batch.children
                    .iter()
                    .filter(|child| {
                        matches!(
                            child.view.state.as_str(),
                            "Queued" | "Checking" | "AwaitingConflictDecision" | "Running" | "Cancelling"
                        )
                    })
                    .count()
            })
            .sum(),
        finished_count: state
            .batches
            .iter()
            .filter(|batch| is_terminal_state(&batch.view.state))
            .count(),
        batch_count: state.batches.len(),
    }
}

fn next_runnable_child(state: &TransferState) -> Option<(usize, usize)> {
    for (batch_index, batch) in state.batches.iter().enumerate() {
        if is_terminal_state(&batch.view.state)
            || batch.view.state == "AwaitingConflictDecision"
            || batch.view.state == "PausedDisconnected"
        {
            continue;
        }

        if let Some(child_index) = batch.children.iter().position(|child| child.view.state == "Queued") {
            return Some((batch_index, child_index));
        }
    }

    None
}

fn cancel_batch_by_id(state: &mut TransferState, job_id: &str) -> bool {
    let Some(batch) = state.batches.iter_mut().find(|batch| batch.id == job_id) else {
        return false;
    };

    batch.cancel_requested = true;
    batch.pause_message = None;
    if let Some(active_child) = batch
        .children
        .iter_mut()
        .find(|child| matches!(child.view.state.as_str(), "Checking" | "Running" | "Cancelling"))
    {
        active_child.view.state = "Cancelling".into();
        active_child.view.can_cancel = false;
        if let Some(flag) = state.cancel_flags.get(&active_child.id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    for child in &mut batch.children {
        if matches!(child.view.state.as_str(), "Queued" | "AwaitingConflictDecision") {
            child.view.state = "Cancelled".into();
            child.view.conflict = None;
            child.view.error_message = None;
            child.view.can_cancel = false;
            child.view.can_retry = true;
        }
    }

    refresh_batch_aggregate(batch);
    true
}

fn cancel_child_by_id(state: &mut TransferState, job_id: &str) {
    let cancel_flag = state.cancel_flags.get(job_id).cloned();
    let Some((batch_index, child_index)) = find_child_indices(state, job_id) else {
        return;
    };
    let batch = &mut state.batches[batch_index];
    let child = &mut batch.children[child_index];

    match child.view.state.as_str() {
        "Queued" | "Checking" | "AwaitingConflictDecision" => {
            child.view.state = "Cancelled".into();
            child.view.error_message = None;
            child.view.conflict = None;
            child.view.can_cancel = false;
            child.view.can_retry = true;
            if let Some(flag) = cancel_flag.as_ref() {
                flag.store(true, Ordering::Relaxed);
            }
        }
        "Running" | "Cancelling" => {
            child.view.state = "Cancelling".into();
            child.view.can_cancel = false;
            if let Some(flag) = cancel_flag.as_ref() {
                flag.store(true, Ordering::Relaxed);
            }
        }
        _ => {}
    }

    refresh_batch_aggregate(batch);
}

fn retry_batch_by_id(state: &mut TransferState, job_id: &str) -> bool {
    let Some(batch) = state.batches.iter_mut().find(|batch| batch.id == job_id) else {
        return false;
    };

    batch.cancel_requested = false;
    batch.pause_message = None;
    batch.conflict_policy = ConflictPolicy::Ask;
    batch.view.error_message = None;
    for child in &mut batch.children {
        if child.view.state != "Succeeded" {
            reset_child_for_retry(child);
        }
    }
    refresh_batch_aggregate(batch);
    true
}

fn retry_child_by_id(state: &mut TransferState, job_id: &str) {
    let Some((batch_index, child_index)) = find_child_indices(state, job_id) else {
        return;
    };
    let batch = &mut state.batches[batch_index];
    let child = &mut batch.children[child_index];
    batch.cancel_requested = false;
    batch.pause_message = None;
    batch.view.error_message = None;
    reset_child_for_retry(child);
    refresh_batch_aggregate(batch);
}

fn reset_child_for_retry(child: &mut TransferChildRecord) {
    child.overwrite_approved = false;
    child.view.state = "Queued".into();
    child.view.error_message = None;
    child.view.conflict = None;
    child.view.rate = None;
    child.view.can_cancel = true;
    child.view.can_retry = false;
    child.view.bytes_transferred = 0;
    child.view.progress_percent = Some(progress_fraction(0, child.view.bytes_total));
}

fn find_child_indices(
    state: &TransferState,
    job_id: &str,
) -> Option<(usize, usize)> {
    for (batch_index, batch) in state.batches.iter().enumerate() {
        if let Some(child_index) = batch.children.iter().position(|child| child.id == job_id) {
            return Some((batch_index, child_index));
        }
    }
    None
}

fn find_conflict_target_indices(state: &TransferState, job_id: &str) -> Option<(usize, usize)> {
    if let Some(indices) = find_child_indices(state, job_id) {
        return Some(indices);
    }

    state.batches.iter().enumerate().find_map(|(batch_index, batch)| {
        if batch.id != job_id {
            return None;
        }

        batch.children
            .iter()
            .position(|child| child.view.state == "AwaitingConflictDecision")
            .map(|child_index| (batch_index, child_index))
    })
}

fn finalize_cancelled_batches(state: &mut TransferState) {
    for batch in &mut state.batches {
        if batch.cancel_requested
            && !batch
                .children
                .iter()
                .any(|child| matches!(child.view.state.as_str(), "Checking" | "Running" | "Cancelling"))
        {
            mark_remaining_children_cancelled(batch);
            refresh_batch_aggregate(batch);
        }
    }
}

fn mark_remaining_children_cancelled(batch: &mut TransferBatchRecord) {
    for child in &mut batch.children {
        if matches!(child.view.state.as_str(), "Queued" | "AwaitingConflictDecision" | "Checking") {
            child.view.state = "Cancelled".into();
            child.view.conflict = None;
            child.view.error_message = None;
            child.view.can_cancel = false;
            child.view.can_retry = true;
        }
    }
}

fn pause_batches_for_disconnect(state: &mut TransferState, message: &str) {
    for batch in &mut state.batches {
        if is_terminal_state(&batch.view.state) {
            continue;
        }

        if batch
            .children
            .iter()
            .any(|child| matches!(child.view.state.as_str(), "Queued" | "AwaitingConflictDecision" | "Checking" | "Running" | "Cancelling"))
        {
            batch.pause_message = Some(message.into());
            refresh_batch_aggregate(batch);
        }
    }
}

fn refresh_batch_aggregate(batch: &mut TransferBatchRecord) {
    let total_files = batch
        .children
        .iter()
        .filter(|child| child.item_kind == PlannedItemKind::File)
        .count();
    let total_directories = batch
        .children
        .iter()
        .filter(|child| child.item_kind == PlannedItemKind::Directory)
        .count();
    let completed_files = batch
        .children
        .iter()
        .filter(|child| child.item_kind == PlannedItemKind::File && child.view.state == "Succeeded")
        .count();
    let failed_files = batch
        .children
        .iter()
        .filter(|child| child.item_kind == PlannedItemKind::File && child.view.state == "Failed")
        .count();
    let skipped_files = batch
        .children
        .iter()
        .filter(|child| {
            child.item_kind == PlannedItemKind::File
                && matches!(child.view.state.as_str(), "Skipped" | "Cancelled")
        })
        .count();

    batch.view.summary = Some(TransferJobSummary {
        total_files,
        total_directories,
        completed_files,
        failed_files,
        skipped_files,
    });

    let conflict_child = batch
        .children
        .iter()
        .find(|child| child.view.state == "AwaitingConflictDecision");
    let active_child = batch.children.iter().find(|child| {
        matches!(
            child.view.state.as_str(),
            "Checking" | "Running" | "Cancelling"
        )
    });
    batch.view.current_item_label = active_child.map(|child| child.view.name.clone());
    batch.view.rate = active_child.and_then(|child| child.view.rate.clone());
    batch.view.conflict = conflict_child.and_then(|child| child.view.conflict.clone());

    let mut total_bytes = 0_u64;
    let mut all_known = true;
    let mut transferred_bytes = 0_u64;
    for child in &batch.children {
        if child.item_kind == PlannedItemKind::Directory {
            continue;
        }

        match child.view.bytes_total {
            Some(bytes) => total_bytes = total_bytes.saturating_add(bytes),
            None => all_known = false,
        }

        transferred_bytes = transferred_bytes.saturating_add(match child.view.state.as_str() {
            "Succeeded" => child.view.bytes_total.unwrap_or(child.view.bytes_transferred),
            "Running" | "Checking" | "Cancelling" => child.view.bytes_transferred,
            _ => 0,
        });
    }

    batch.view.bytes_total = if all_known { Some(total_bytes) } else { None };
    batch.view.bytes_transferred = transferred_bytes;
    batch.view.progress_percent = Some(if all_known {
        progress_fraction(transferred_bytes, Some(total_bytes))
    } else {
        progress_from_counts(batch)
    });

    let has_pending = batch
        .children
        .iter()
        .any(|child| matches!(child.view.state.as_str(), "Queued" | "Checking" | "Running" | "Cancelling" | "AwaitingConflictDecision"));
    let all_cancelled = batch
        .children
        .iter()
        .all(|child| matches!(child.view.state.as_str(), "Cancelled" | "Skipped"));
    let any_failed = batch.children.iter().any(|child| child.view.state == "Failed");
    let any_cancelled = batch
        .children
        .iter()
        .any(|child| matches!(child.view.state.as_str(), "Cancelled" | "Skipped"));
    let any_succeeded = batch.children.iter().any(|child| child.view.state == "Succeeded");

    if batch.pause_message.is_some() {
        batch.view.state = "PausedDisconnected".into();
        batch.view.error_message = batch.pause_message.clone();
        batch.view.can_cancel = true;
        batch.view.can_retry = true;
    } else if batch.cancel_requested {
        if has_pending {
            batch.view.state = "Cancelling".into();
            batch.view.error_message = None;
            batch.view.can_cancel = false;
            batch.view.can_retry = false;
        } else {
            batch.view.state = "Cancelled".into();
            batch.view.error_message = None;
            batch.view.can_cancel = false;
            batch.view.can_retry = !all_cancelled;
        }
    } else if batch.children.iter().any(|child| child.view.state == "AwaitingConflictDecision") {
        batch.view.state = "AwaitingConflictDecision".into();
        batch.view.error_message = None;
        batch.view.can_cancel = true;
        batch.view.can_retry = false;
    } else if batch
        .children
        .iter()
        .any(|child| matches!(child.view.state.as_str(), "Checking" | "Running" | "Cancelling"))
    {
        batch.view.state = if batch
            .children
            .iter()
            .any(|child| child.view.state == "Cancelling")
        {
            "Cancelling".into()
        } else {
            "Running".into()
        };
        batch.view.error_message = None;
        batch.view.can_cancel = true;
        batch.view.can_retry = false;
    } else if has_pending {
        batch.view.state = "Queued".into();
        batch.view.error_message = None;
        batch.view.can_cancel = true;
        batch.view.can_retry = false;
    } else if any_failed || any_cancelled {
        batch.view.state = if any_succeeded { "CompletedWithErrors".into() } else if all_cancelled { "Cancelled".into() } else { "Failed".into() };
        batch.view.error_message = Some(batch_completion_message(batch));
        batch.view.can_cancel = false;
        batch.view.can_retry = true;
    } else {
        batch.view.state = "Succeeded".into();
        batch.view.error_message = None;
        batch.view.can_cancel = false;
        batch.view.can_retry = false;
    }
}

fn batch_completion_message(batch: &TransferBatchRecord) -> String {
    let summary = batch.view.summary.clone().unwrap_or(TransferJobSummary {
        total_files: 0,
        total_directories: 0,
        completed_files: 0,
        failed_files: 0,
        skipped_files: 0,
    });
    if batch.view.state == "PausedDisconnected" {
        return batch
            .pause_message
            .clone()
            .unwrap_or_else(|| "Reconnect and retry the batch to continue transferring.".into());
    }

    let mut parts = Vec::new();
    parts.push(format!("{} completed", summary.completed_files));
    if summary.failed_files > 0 {
        parts.push(format!("{} failed", summary.failed_files));
    }
    if summary.skipped_files > 0 {
        parts.push(format!("{} skipped", summary.skipped_files));
    }
    parts.join(", ")
}

fn progress_from_counts(batch: &TransferBatchRecord) -> u8 {
    let total = batch.children.len() as f32;
    if total == 0.0 {
        return 100;
    }

    let mut completed = 0_f32;
    for child in &batch.children {
        completed += match child.view.state.as_str() {
            "Succeeded" | "Failed" | "Skipped" | "Cancelled" => 1.0,
            "Running" | "Checking" | "Cancelling" => child.view.progress_percent.unwrap_or(0) as f32 / 100.0,
            _ => 0.0,
        };
    }

    ((completed / total) * 100.0).round().clamp(0.0, 100.0) as u8
}

fn is_terminal_state(state: &str) -> bool {
    matches!(state, "Succeeded" | "Failed" | "Cancelled" | "CompletedWithErrors")
}

fn is_session_loss_message(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("ssh session ended") || message.contains("connection lost after idle")
}

fn contextualize_conflict(
    mut conflict: TransferConflict,
    source_path: &str,
    destination_path: &str,
    source_kind: &str,
) -> TransferConflict {
    conflict.source_kind = source_kind.into();
    conflict.source_path = source_path.into();
    conflict.source_name = Path::new(source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(source_path)
        .into();
    conflict.destination_path = destination_path.into();
    conflict.destination_name = Path::new(destination_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(destination_path)
        .into();
    conflict.conflict_kind = if conflict.destination_kind == "dir" && source_kind != "dir" {
        "typeMismatch".into()
    } else if conflict.destination_kind == "file" && source_kind == "file" {
        "fileExists".into()
    } else if conflict.destination_kind == "dir" && source_kind == "dir" {
        "dirExists".into()
    } else {
        "unknown".into()
    };
    conflict
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
            source_kind: "unknown".into(),
            source_name: String::new(),
            source_path: String::new(),
            destination_name: destination_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .into(),
            destination_path: destination_path.display().to_string(),
            conflict_kind: if metadata.file_type().is_dir() {
                "typeMismatch".into()
            } else {
                "fileExists".into()
            },
            can_overwrite: !metadata.file_type().is_dir(),
            apply_to_remaining: true,
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
            source_kind: "unknown".into(),
            source_name: String::new(),
            source_path: String::new(),
            destination_name: Path::new(destination_path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .into(),
            destination_path: destination_path.into(),
            conflict_kind: if metadata.is_dir() {
                "typeMismatch".into()
            } else {
                "fileExists".into()
            },
            can_overwrite: !metadata.is_dir(),
            apply_to_remaining: true,
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

async fn ensure_remote_directory_chain(sftp: &SftpSession, path: &str) -> Result<()> {
    let normalized = normalize_remote_directory_path(path);
    if normalized == "/" {
        return Ok(());
    }

    let mut current = String::from("/");
    for segment in normalized.split('/').filter(|segment| !segment.is_empty()) {
        current = join_remote_path(&current, segment);

        match inspect_remote_conflict(sftp, &current).await? {
            Some(conflict) if conflict.destination_kind == "dir" => continue,
            Some(_) => {
                bail!("Warp cannot create a remote folder because `{current}` already exists as a file.")
            }
            None => {
                sftp.create_dir(&current)
                    .await
                    .map_err(|error| anyhow!(classify_transfer_remote_error("create_dir", &current, error)))?;
            }
        }
    }

    Ok(())
}

fn temp_local_path(destination_path: &Path, job_id: &str) -> PathBuf {
    let name = destination_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("transfer");
    destination_path.with_file_name(format!(".{name}.warp-part-{job_id}"))
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

fn normalize_remote_directory_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        "/".into()
    } else {
        format!("/{}", trimmed.trim_matches('/'))
    }
}

fn parent_remote_path(path: &str) -> String {
    RemoteSftpEngine::parent_path(path).unwrap_or_else(|| "/".into())
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

fn classify_transfer_remote_error(action: &str, path: &str, error: SftpError) -> String {
    match error {
        SftpError::Status(status) => match status.status_code {
            russh_sftp::protocol::StatusCode::PermissionDenied => match action {
                "create_dir" => format!("Permission denied while preparing remote folder `{path}`."),
                "create_file" | "flush_file" | "replace_file" | "finalize_upload" => {
                    format!("Permission denied while writing remote file `{path}`.")
                }
                _ => format!("Permission denied while accessing remote path `{path}`."),
            },
            russh_sftp::protocol::StatusCode::NoSuchFile => match action {
                "create_dir" => format!("The remote parent path for `{path}` is no longer available."),
                "create_file" | "finalize_upload" => {
                    format!("The remote parent path for `{path}` is no longer available, so Warp could not upload the file.")
                }
                "inspect_file" | "open_file" => {
                    format!("The remote file `{path}` is no longer available.")
                }
                _ => format!("The remote path `{path}` is no longer available."),
            },
            russh_sftp::protocol::StatusCode::ConnectionLost | russh_sftp::protocol::StatusCode::NoConnection => {
                format!("The SSH session ended while accessing remote path `{path}`.")
            }
            russh_sftp::protocol::StatusCode::Failure => {
                let message = status.error_message.trim();
                if message.is_empty() || message.eq_ignore_ascii_case("failure") {
                    match action {
                        "create_dir" => format!("The server could not create remote folder `{path}`."),
                        "create_file" => format!("The server could not create the remote upload target `{path}`."),
                        "flush_file" => format!("The server could not finish writing remote file `{path}`."),
                        "close_file" => format!("The server could not close remote file `{path}` cleanly after writing."),
                        "replace_file" => format!("The server could not replace the existing remote file at `{path}`."),
                        "finalize_upload" => format!("The server could not finalize the upload for `{path}`."),
                        "inspect_file" | "open_file" => format!("The server could not access remote file `{path}`."),
                        _ => format!("The server could not complete the remote operation for `{path}`."),
                    }
                } else {
                    message.into()
                }
            }
            _ => {
                let message = status.error_message.trim();
                if message.is_empty() {
                    format!("Remote operation failed for `{path}` ({})", status.status_code)
                } else {
                    format!("{} ({})", message, status.status_code)
                }
            }
        },
        other => friendly_remote_transfer_error("access the remote path", other),
    }
}

impl TransferDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Upload => "Upload",
            Self::Download => "Download",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{batch_display_name, format_rate, join_remote_path, progress_fraction};
    use crate::models::TransferSelectionItem;

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

    #[test]
    fn reports_progress_for_zero_byte_items() {
        assert_eq!(progress_fraction(0, Some(0)), 100);
    }

    #[test]
    fn labels_multi_entry_batches() {
        let entries = vec![
            TransferSelectionItem {
                path: "/tmp/a".into(),
                name: "a".into(),
                kind: "file".into(),
            },
            TransferSelectionItem {
                path: "/tmp/b".into(),
                name: "b".into(),
                kind: "file".into(),
            },
        ];
        assert_eq!(batch_display_name(&entries), "2 items");
    }
}
