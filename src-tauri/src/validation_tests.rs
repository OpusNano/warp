#![cfg(test)]

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use russh_sftp::client::{SftpSession, error::Error as SftpError};
use sha2::{Digest, Sha256};
use tauri::{Runtime, test::mock_app};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    time::sleep,
};

use crate::{
    models::{
        ConnectAuth, ConnectRequest, CreateRemoteDirectoryRequest, DeleteRemoteEntryRequest,
        QueueDownloadRequest, QueueUploadRequest, RenameRemoteEntryRequest, TrustDecision,
        TransferConflictResolution, TransferJob, TransferSelectionItem,
    },
    session::SessionManager,
    transfer::TransferManager,
};

const HOST: &str = "192.168.178.40";
const PORT: u16 = 22;
const USERNAME: &str = "devtest";
const PASSWORD: &str = "devtest";
const KEY_PASSPHRASE: &str = "warp-pass";

#[derive(Clone, Copy, Debug)]
struct FixtureCapabilities {
    remote_mode_supported: bool,
    remote_symlink_supported: bool,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validates_real_host_transfer_and_session_engine() -> Result<()> {
    let run_id = format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let temp_root = env::temp_dir().join(format!("warp-validation-{run_id}"));
    fs::create_dir_all(&temp_root).await?;

    let app = mock_app();
    let trust_store_a = temp_root.join("trust-a");
    let trust_store_b = temp_root.join("trust-b");
    fs::create_dir_all(&trust_store_a).await?;
    fs::create_dir_all(&trust_store_b).await?;

    let manager_a = Arc::new(SessionManager::new(app.handle().clone(), trust_store_a.clone()));

    let trust_prompt = manager_a.connect(password_request()).await?;
    assert_eq!(trust_prompt.session.connection_state, "Awaiting trust");
    assert!(trust_prompt.trust_prompt.is_some(), "expected first-seen trust prompt");

    let trust_cancelled = manager_a.resolve_trust(TrustDecision { trust: false }).await?;
    assert_eq!(trust_cancelled.session.connection_state, "Disconnected");
    assert!(
        trust_cancelled
            .session
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("Connection cancelled before trust was granted"),
        "unexpected trust cancel message: {:?}",
        trust_cancelled.session.last_error
    );

    let trust_again = manager_a.connect(password_request()).await?;
    assert!(trust_again.trust_prompt.is_some(), "expected trust prompt on reconnect before trust acceptance");
    let trusted = manager_a.resolve_trust(TrustDecision { trust: true }).await?;
    assert_eq!(trusted.session.connection_state, "Connected");
    assert!(trusted.trust_prompt.is_none());
    manager_a.disconnect().await?;

    let manager_b = Arc::new(SessionManager::new(app.handle().clone(), trust_store_a.clone()));
    let trusted_reconnect = manager_b.connect(password_request()).await?;
    assert_eq!(trusted_reconnect.session.connection_state, "Connected");
    assert!(trusted_reconnect.trust_prompt.is_none(), "trusted host should not prompt again");

    let wrong_password = manager_b
        .connect(ConnectRequest {
            host: HOST.into(),
            port: PORT,
            username: USERNAME.into(),
            auth: ConnectAuth::Password {
                password: "definitely-wrong".into(),
            },
        })
        .await?;
    assert_eq!(wrong_password.session.connection_state, "Disconnected");
    assert!(wrong_password.session.last_error.is_some(), "wrong password should fail");

    let manager = Arc::new(SessionManager::new(app.handle().clone(), trust_store_b.clone()));
    let connected = connect_and_trust(&manager, password_request()).await?;
    let remote_home = connected.remote_pane.location.clone();

    let key_dir = temp_root.join("keys");
    fs::create_dir_all(&key_dir).await?;
    let private_key_path = key_dir.join("id_ed25519");
    generate_test_keypair(&private_key_path)?;
    let public_key = fs::read_to_string(private_key_path.with_extension("pub")).await?;
    let authorized_keys_path = join_remote(&remote_home, ".ssh/authorized_keys");
    let original_authorized_keys = append_authorized_key(&manager, &remote_home, &authorized_keys_path, &public_key).await?;

    manager.disconnect().await?;
    let wrong_key_path = manager
        .connect(ConnectRequest {
            host: HOST.into(),
            port: PORT,
            username: USERNAME.into(),
            auth: ConnectAuth::Key {
                private_key_path: key_dir.join("missing").display().to_string(),
                passphrase: None,
            },
        })
        .await?;
    assert_eq!(wrong_key_path.session.connection_state, "Disconnected");
    assert!(wrong_key_path.session.last_error.is_some());

    let wrong_passphrase = manager
        .connect(ConnectRequest {
            host: HOST.into(),
            port: PORT,
            username: USERNAME.into(),
            auth: ConnectAuth::Key {
                private_key_path: private_key_path.display().to_string(),
                passphrase: Some("wrong-passphrase".into()),
            },
        })
        .await?;
    assert_eq!(wrong_passphrase.session.connection_state, "Disconnected");
    assert!(wrong_passphrase.session.last_error.is_some());

    let key_connected = connect_and_trust(
        &manager,
        ConnectRequest {
            host: HOST.into(),
            port: PORT,
            username: USERNAME.into(),
            auth: ConnectAuth::Key {
                private_key_path: private_key_path.display().to_string(),
                passphrase: Some(KEY_PASSPHRASE.into()),
            },
        },
    )
    .await?;
    assert_eq!(key_connected.session.connection_state, "Connected");

    for _ in 0..3 {
        manager.disconnect().await?;
        let cycle = connect_and_trust(&manager, password_request()).await?;
        assert_eq!(cycle.session.connection_state, "Connected");
    }

    let transfer_manager = Arc::new(TransferManager::new(app.handle().clone(), manager.clone()));
    let remote_root = join_remote(&remote_home, &format!("warp-validation-{run_id}"));
    let local_root = temp_root.join("local");
    let local_src = local_root.join("src");
    let local_downloads = local_root.join("downloads");
    fs::create_dir_all(&local_src).await?;
    fs::create_dir_all(&local_downloads).await?;

    build_local_fixtures(&local_src).await?;
    let capabilities = build_remote_fixtures(&manager, &remote_root).await?;

    let listing = manager.open_remote_directory(remote_root.clone()).await?;
    assert!(names(&listing.remote_pane).contains("nested"));
    assert!(names(&listing.remote_pane).contains(".hidden-remote"));
    if capabilities.remote_symlink_supported {
        assert!(listing.remote_pane.entries.iter().any(|entry| entry.name == "symbolic-link"));
    } else {
        println!("skipping symlink listing assertion; remote symlink creation is not supported on this host");
    }

    let nested = manager.open_remote_directory(join_remote(&remote_root, "nested")).await?;
    assert_eq!(nested.remote_pane.location, join_remote(&remote_root, "nested"));
    let parent = manager.go_up_remote_directory().await?;
    assert_eq!(parent.remote_pane.location, remote_root);

    if capabilities.remote_mode_supported {
        let unreadable = manager
            .open_remote_directory(join_remote(&remote_root, "unreadable-dir"))
            .await?;
        assert!(unreadable.session.last_error.is_some(), "expected unreadable directory error");
    } else {
        println!("skipping unreadable-directory assertion; remote chmod via SFTP is not supported on this host");
    }

    let missing = manager
        .open_remote_directory(join_remote(&remote_root, "does-not-exist"))
        .await?;
    assert!(missing.session.last_error.is_some(), "expected missing path error");

    let mkdir_snapshot = manager
        .create_remote_directory(CreateRemoteDirectoryRequest {
            parent_path: remote_root.clone(),
            name: "created-here".into(),
        })
        .await?;
    assert!(names(&mkdir_snapshot.remote_pane).contains("created-here"));

    create_remote_file(&manager, &join_remote(&remote_root, "rename-me.txt"), b"rename me").await?;
    let rename_file = manager
        .rename_remote_entry(RenameRemoteEntryRequest {
            parent_path: remote_root.clone(),
            entry_name: "rename-me.txt".into(),
            new_name: "renamed-file.txt".into(),
        })
        .await?;
    assert!(names(&rename_file.remote_pane).contains("renamed-file.txt"));

    let rename_dir = manager
        .rename_remote_entry(RenameRemoteEntryRequest {
            parent_path: remote_root.clone(),
            entry_name: "rename-dir".into(),
            new_name: "renamed-dir".into(),
        })
        .await?;
    assert!(names(&rename_dir.remote_pane).contains("renamed-dir"));

    create_remote_file(&manager, &join_remote(&remote_root, "conflict-a.txt"), b"a").await?;
    create_remote_file(&manager, &join_remote(&remote_root, "conflict-b.txt"), b"b").await?;
    let rename_conflict = manager
        .rename_remote_entry(RenameRemoteEntryRequest {
            parent_path: remote_root.clone(),
            entry_name: "conflict-a.txt".into(),
            new_name: "conflict-b.txt".into(),
        })
        .await?;
    assert!(rename_conflict.session.last_error.is_some(), "expected rename conflict to fail");

    let delete_file = manager
        .delete_remote_entry(DeleteRemoteEntryRequest {
            parent_path: remote_root.clone(),
            entry_name: "conflict-a.txt".into(),
            entry_kind: "file".into(),
            recursive: false,
        })
        .await?;
    assert!(delete_file.prompt.is_none());

    create_remote_file(&manager, &join_remote(&remote_root, "delete-tree/child.txt"), b"child").await?;
    let delete_prompt = manager
        .delete_remote_entry(DeleteRemoteEntryRequest {
            parent_path: remote_root.clone(),
            entry_name: "delete-tree".into(),
            entry_kind: "dir".into(),
            recursive: false,
        })
        .await?;
    assert!(delete_prompt.prompt.as_ref().is_some_and(|prompt| prompt.requires_recursive));
    let delete_recursive = manager
        .delete_remote_entry(DeleteRemoteEntryRequest {
            parent_path: remote_root.clone(),
            entry_name: "delete-tree".into(),
            entry_kind: "dir".into(),
            recursive: true,
        })
        .await?;
    assert!(delete_recursive.snapshot.session.last_error.is_none());

    create_remote_file(&manager, &join_remote(&remote_root, "batch-delete/file-a.txt"), b"a").await?;
    create_remote_file(&manager, &join_remote(&remote_root, "batch-delete/file-b.txt"), b"b").await?;
    create_remote_file(&manager, &join_remote(&remote_root, "batch-delete/folder/child.txt"), b"c").await?;
    let batch_delete_prompt = manager
        .delete_remote_entries(crate::models::DeleteRemoteEntriesRequest {
            parent_path: join_remote(&remote_root, "batch-delete"),
            entries: vec![
                crate::models::DeleteRemoteEntryTarget {
                    entry_name: "file-a.txt".into(),
                    entry_kind: "file".into(),
                },
                crate::models::DeleteRemoteEntryTarget {
                    entry_name: "folder".into(),
                    entry_kind: "dir".into(),
                },
            ],
            recursive: false,
        })
        .await?;
    assert!(batch_delete_prompt.prompt.as_ref().is_some_and(|prompt| prompt.entries.len() == 2));
    let batch_delete_done = manager
        .delete_remote_entries(crate::models::DeleteRemoteEntriesRequest {
            parent_path: join_remote(&remote_root, "batch-delete"),
            entries: vec![
                crate::models::DeleteRemoteEntryTarget {
                    entry_name: "file-a.txt".into(),
                    entry_kind: "file".into(),
                },
                crate::models::DeleteRemoteEntryTarget {
                    entry_name: "folder".into(),
                    entry_kind: "dir".into(),
                },
            ],
            recursive: true,
        })
        .await?;
    assert!(batch_delete_done.snapshot.session.last_error.is_none());
    assert!(!remote_file_exists(&manager, &join_remote(&remote_root, "batch-delete/file-a.txt")).await?);
    assert!(!remote_file_exists(&manager, &join_remote(&remote_root, "batch-delete/folder")).await?);

    if capabilities.remote_mode_supported {
        let denied_mkdir = manager
            .create_remote_directory(CreateRemoteDirectoryRequest {
                parent_path: join_remote(&remote_root, "unwritable-dir"),
                name: "nope".into(),
            })
            .await?;
        assert!(denied_mkdir.session.last_error.is_some(), "expected permission denied mkdir");
    } else {
        println!("skipping unwritable mkdir assertion; remote chmod via SFTP is not supported on this host");
    }

    let upload_target = join_remote(&remote_root, "uploads single");
    ensure_remote_dir(&manager, &upload_target).await?;
    let single_upload_snapshot = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![selection_item(local_src.join("alpha.txt"), "file")],
            remote_directory: upload_target.clone(),
        })
        .await?;
    let single_upload_batch = latest_batch_id(&single_upload_snapshot)?;
    wait_for_batch_state(&transfer_manager, &single_upload_batch, &["Succeeded"]).await?;
    assert_remote_hash_eq_local(
        &manager,
        &join_remote(&upload_target, "alpha.txt"),
        &local_src.join("alpha.txt"),
    )
    .await?;

    let single_download_snapshot = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![TransferSelectionItem {
                path: join_remote(&remote_root, "remote-one.txt"),
                name: "remote-one.txt".into(),
                kind: "file".into(),
            }],
            local_directory: local_downloads.display().to_string(),
        })
        .await?;
    let single_download_batch = latest_batch_id(&single_download_snapshot)?;
    wait_for_batch_state(&transfer_manager, &single_download_batch, &["Succeeded"]).await?;
    assert_local_hash_eq_remote(
        &manager,
        &local_downloads.join("remote-one.txt"),
        &join_remote(&remote_root, "remote-one.txt"),
    )
    .await?;

    let multi_upload_target = join_remote(&remote_root, "multi upload");
    ensure_remote_dir(&manager, &multi_upload_target).await?;
    let multi_start = Instant::now();
    let multi_upload = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![
                selection_item(local_src.join("alpha.txt"), "file"),
                selection_item(local_src.join("zero.bin"), "file"),
                selection_item(local_src.join("space name.txt"), "file"),
                selection_item(local_src.join("unicodé.txt"), "file"),
            ],
            remote_directory: multi_upload_target.clone(),
        })
        .await?;
    println!("multi-file upload planning elapsed: {:?}", multi_start.elapsed());
    let multi_upload_batch = latest_batch_id(&multi_upload)?;
    wait_for_batch_state(&transfer_manager, &multi_upload_batch, &["Succeeded"]).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&multi_upload_target, "zero.bin"), &local_src.join("zero.bin")).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&multi_upload_target, "unicodé.txt"), &local_src.join("unicodé.txt")).await?;

    let multi_download_dir = local_downloads.join("multi-download");
    fs::create_dir_all(&multi_download_dir).await?;
    let multi_download = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![
                TransferSelectionItem {
                    path: join_remote(&remote_root, "remote-one.txt"),
                    name: "remote-one.txt".into(),
                    kind: "file".into(),
                },
                TransferSelectionItem {
                    path: join_remote(&remote_root, "remote zero.bin"),
                    name: "remote zero.bin".into(),
                    kind: "file".into(),
                },
                TransferSelectionItem {
                    path: join_remote(&remote_root, "remote unicodé.txt"),
                    name: "remote unicodé.txt".into(),
                    kind: "file".into(),
                },
            ],
            local_directory: multi_download_dir.display().to_string(),
        })
        .await?;
    let multi_download_batch = latest_batch_id(&multi_download)?;
    wait_for_batch_state(&transfer_manager, &multi_download_batch, &["Succeeded"]).await?;
    assert_local_hash_eq_remote(&manager, &multi_download_dir.join("remote unicodé.txt"), &join_remote(&remote_root, "remote unicodé.txt")).await?;

    let download_conflict_dir = local_downloads.join("download-conflict");
    fs::create_dir_all(&download_conflict_dir).await?;
    write_local_file(&download_conflict_dir.join("remote-one.txt"), b"old-remote-one\n").await?;
    write_local_file(&download_conflict_dir.join("remote unicodé.txt"), b"old-remote-unicode\n").await?;
    let conflict_download = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![
                TransferSelectionItem {
                    path: join_remote(&remote_root, "remote-one.txt"),
                    name: "remote-one.txt".into(),
                    kind: "file".into(),
                },
                TransferSelectionItem {
                    path: join_remote(&remote_root, "remote unicodé.txt"),
                    name: "remote unicodé.txt".into(),
                    kind: "file".into(),
                },
            ],
            local_directory: download_conflict_dir.display().to_string(),
        })
        .await?;
    let sequence_before_download_resolve = conflict_download.sequence;
    let conflict_download_batch = latest_batch_id(&conflict_download)?;
    let overwrite_download_child = wait_for_conflict_child(&transfer_manager, &conflict_download_batch).await?;
    let resolved_download = transfer_manager
        .resolve_conflict(
            &overwrite_download_child.id,
            TransferConflictResolution {
                action: "overwriteAll".into(),
            },
        )
        .await?;
    assert!(resolved_download.sequence > sequence_before_download_resolve);
    wait_for_batch_state(&transfer_manager, &conflict_download_batch, &["Succeeded"]).await?;
    assert_local_hash_eq_remote(&manager, &download_conflict_dir.join("remote-one.txt"), &join_remote(&remote_root, "remote-one.txt")).await?;
    let followup_download = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![TransferSelectionItem {
                path: join_remote(&remote_root, "remote zero.bin"),
                name: "remote zero.bin".into(),
                kind: "file".into(),
            }],
            local_directory: download_conflict_dir.display().to_string(),
        })
        .await?;
    assert!(followup_download.sequence > resolved_download.sequence);
    let followup_download_batch = latest_batch_id(&followup_download)?;
    wait_for_batch_state(&transfer_manager, &followup_download_batch, &["Succeeded"]).await?;
    assert_local_hash_eq_remote(&manager, &download_conflict_dir.join("remote zero.bin"), &join_remote(&remote_root, "remote zero.bin")).await?;

    let recursive_upload_target = join_remote(&remote_root, "recursive upload");
    ensure_remote_dir(&manager, &recursive_upload_target).await?;
    let recursive_upload = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![selection_item(local_src.join("dir upload"), "dir")],
            remote_directory: recursive_upload_target.clone(),
        })
        .await?;
    let recursive_upload_batch = latest_batch_id(&recursive_upload)?;
    wait_for_batch_state(&transfer_manager, &recursive_upload_batch, &["Succeeded"]).await?;
    assert_remote_tree_matches_local(
        &local_src.join("dir upload"),
        &manager,
        &join_remote(&recursive_upload_target, "dir upload"),
    )
    .await?;

    let recursive_download_dir = local_downloads.join("recursive-download");
    fs::create_dir_all(&recursive_download_dir).await?;
    let recursive_download = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![TransferSelectionItem {
                path: join_remote(&remote_root, "download dir"),
                name: "download dir".into(),
                kind: "dir".into(),
            }],
            local_directory: recursive_download_dir.display().to_string(),
        })
        .await?;
    let recursive_download_batch = latest_batch_id(&recursive_download)?;
    wait_for_batch_state(&transfer_manager, &recursive_download_batch, &["Succeeded"]).await?;
    assert_remote_tree_matches_local(
        &local_downloads.join("recursive-download/download dir"),
        &manager,
        &join_remote(&remote_root, "download dir"),
    )
    .await?;

    let mixed_upload_target = join_remote(&remote_root, "mixed batch");
    ensure_remote_dir(&manager, &mixed_upload_target).await?;
    let mixed_upload = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![
                selection_item(local_src.join("alpha.txt"), "file"),
                selection_item(local_src.join("dir upload"), "dir"),
            ],
            remote_directory: mixed_upload_target.clone(),
        })
        .await?;
    let mixed_batch = latest_batch_id(&mixed_upload)?;
    wait_for_batch_state(&transfer_manager, &mixed_batch, &["Succeeded"]).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&mixed_upload_target, "alpha.txt"), &local_src.join("alpha.txt")).await?;
    assert_remote_tree_matches_local(
        &local_src.join("dir upload"),
        &manager,
        &join_remote(&mixed_upload_target, "dir upload"),
    )
    .await?;

    let conflict_skip_target = join_remote(&remote_root, "conflict skipall");
    ensure_remote_dir(&manager, &conflict_skip_target).await?;
    create_remote_file(&manager, &join_remote(&conflict_skip_target, "alpha.txt"), b"keep-alpha").await?;
    create_remote_file(&manager, &join_remote(&conflict_skip_target, "space name.txt"), b"keep-space").await?;
    let conflict_skip = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![
                selection_item(local_src.join("alpha.txt"), "file"),
                selection_item(local_src.join("space name.txt"), "file"),
            ],
            remote_directory: conflict_skip_target.clone(),
        })
        .await?;
    let conflict_skip_batch = latest_batch_id(&conflict_skip)?;
    let conflict_child = wait_for_conflict_child(&transfer_manager, &conflict_skip_batch).await?;
    assert_eq!(visible_rows_for_batch(&transfer_manager, &conflict_skip_batch).await, 1);
    let conflict = conflict_child.conflict.clone().ok_or_else(|| anyhow!("expected conflict details"))?;
    assert_eq!(conflict.source_name, "alpha.txt");
    assert_eq!(conflict.destination_name, "alpha.txt");
    assert_eq!(conflict.conflict_kind, "fileExists");
    assert_eq!(conflict.destination_path, join_remote(&conflict_skip_target, "alpha.txt"));
    transfer_manager
        .resolve_conflict(
            &conflict_child.id,
            TransferConflictResolution {
                action: "skipAll".into(),
            },
        )
        .await?;
    wait_for_batch_state(&transfer_manager, &conflict_skip_batch, &["CompletedWithErrors", "Failed", "Cancelled"]).await?;
    let cleared = transfer_manager.clear_completed().await;
    assert!(!cleared.jobs.iter().any(|job| job.id == conflict_skip_batch));
    assert_eq!(read_remote_file(&manager, &join_remote(&conflict_skip_target, "alpha.txt")).await?, b"keep-alpha");
    assert_eq!(read_remote_file(&manager, &join_remote(&conflict_skip_target, "space name.txt")).await?, b"keep-space");

    let conflict_overwrite_target = join_remote(&remote_root, "conflict overwriteall");
    ensure_remote_dir(&manager, &conflict_overwrite_target).await?;
    create_remote_file(&manager, &join_remote(&conflict_overwrite_target, "alpha.txt"), b"old-alpha").await?;
    create_remote_file(&manager, &join_remote(&conflict_overwrite_target, "space name.txt"), b"old-space").await?;
    let conflict_overwrite = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![
                selection_item(local_src.join("alpha.txt"), "file"),
                selection_item(local_src.join("space name.txt"), "file"),
            ],
            remote_directory: conflict_overwrite_target.clone(),
        })
        .await?;
    let conflict_overwrite_batch = latest_batch_id(&conflict_overwrite)?;
    let overwrite_child = wait_for_conflict_child(&transfer_manager, &conflict_overwrite_batch).await?;
    let overwrite_resolved = transfer_manager
        .resolve_conflict(
            &overwrite_child.id,
            TransferConflictResolution {
                action: "overwriteAll".into(),
            },
        )
        .await?;
    wait_for_batch_state(&transfer_manager, &conflict_overwrite_batch, &["Succeeded"]).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&conflict_overwrite_target, "alpha.txt"), &local_src.join("alpha.txt")).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&conflict_overwrite_target, "space name.txt"), &local_src.join("space name.txt")).await?;
    let followup_upload = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![selection_item(local_src.join("zero.bin"), "file")],
            remote_directory: conflict_overwrite_target.clone(),
        })
        .await?;
    assert!(followup_upload.sequence > overwrite_resolved.sequence);
    let followup_upload_batch = latest_batch_id(&followup_upload)?;
    wait_for_batch_state(&transfer_manager, &followup_upload_batch, &["Succeeded"]).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&conflict_overwrite_target, "zero.bin"), &local_src.join("zero.bin")).await?;

    let protected_target = join_remote(&remote_root, "protected-upload");
    ensure_remote_dir(&manager, &protected_target).await?;
    if capabilities.remote_mode_supported {
        set_remote_mode(&manager, &protected_target, 0o040555).await?;
        let failed_upload = transfer_manager
            .queue_upload(QueueUploadRequest {
                entries: vec![selection_item(local_src.join("alpha.txt"), "file")],
                remote_directory: protected_target.clone(),
            })
            .await?;
        let failed_batch = latest_batch_id(&failed_upload)?;
        wait_for_batch_state(&transfer_manager, &failed_batch, &["Failed", "CompletedWithErrors"]).await?;
        let failed_child = latest_failed_child(&transfer_manager, &failed_batch).await?;
        set_remote_mode(&manager, &protected_target, 0o040755).await?;
        let retry_child_snapshot = transfer_manager.retry_transfer(&failed_child.id).await;
        assert!(retry_child_snapshot.jobs.iter().any(|job| job.id == failed_child.id));
        wait_for_batch_state(&transfer_manager, &failed_batch, &["Succeeded", "CompletedWithErrors"]).await?;
        assert_remote_hash_eq_local(&manager, &join_remote(&protected_target, "alpha.txt"), &local_src.join("alpha.txt")).await?;
    } else {
        println!("skipping permission-failure upload/retry assertion; remote chmod via SFTP is not supported on this host");
    }

    let protected_download_dir = local_downloads.join("protected-download");
    fs::create_dir_all(&protected_download_dir).await?;
    set_local_mode(&protected_download_dir, 0o555)?;
    let protected_download = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![TransferSelectionItem {
                path: join_remote(&remote_root, "remote-one.txt"),
                name: "remote-one.txt".into(),
                kind: "file".into(),
            }],
            local_directory: protected_download_dir.display().to_string(),
        })
        .await?;
    let protected_download_batch = latest_batch_id(&protected_download)?;
    let protected_download_state = wait_for_batch_state(
        &transfer_manager,
        &protected_download_batch,
        &["Failed", "CompletedWithErrors", "Succeeded"],
    )
    .await?;
    set_local_mode(&protected_download_dir, 0o755)?;
    if matches!(protected_download_state.state.as_str(), "Failed" | "CompletedWithErrors") {
        let protected_download_child = latest_failed_child(&transfer_manager, &protected_download_batch).await?;
        transfer_manager.retry_transfer(&protected_download_child.id).await;
        wait_for_batch_state(&transfer_manager, &protected_download_batch, &["Succeeded", "CompletedWithErrors"]).await?;
        assert_local_hash_eq_remote(&manager, &protected_download_dir.join("remote-one.txt"), &join_remote(&remote_root, "remote-one.txt")).await?;
    } else {
        println!("skipping local permission download failure assertion; local permissions did not block writes in this environment");
    }

    let queued_cancel_target = join_remote(&remote_root, "cancel queued child");
    ensure_remote_dir(&manager, &queued_cancel_target).await?;
    let queued_cancel = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![
                selection_item(local_src.join("large.bin"), "file"),
                selection_item(local_src.join("alpha.txt"), "file"),
            ],
            remote_directory: queued_cancel_target.clone(),
        })
        .await?;
    let queued_cancel_batch = latest_batch_id(&queued_cancel)?;
    let queued_child = wait_for_hidden_child_state(&transfer_manager, &queued_cancel_batch, "Queued", "alpha.txt").await?;
    transfer_manager.cancel_transfer(&queued_child.id).await;
    wait_for_batch_state(&transfer_manager, &queued_cancel_batch, &["CompletedWithErrors", "Succeeded", "Cancelled"]).await?;
    assert!(!remote_file_exists(&manager, &join_remote(&queued_cancel_target, "alpha.txt")).await?);

    let running_cancel_target = join_remote(&remote_root, "cancel running child");
    ensure_remote_dir(&manager, &running_cancel_target).await?;
    let running_cancel = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![selection_item(local_src.join("large.bin"), "file")],
            remote_directory: running_cancel_target.clone(),
        })
        .await?;
    let running_cancel_batch = latest_batch_id(&running_cancel)?;
    let running_child = wait_for_child_state(&transfer_manager, &running_cancel_batch, &["Running", "Checking"]).await?;
    assert_eq!(visible_rows_for_batch(&transfer_manager, &running_cancel_batch).await, 1);
    transfer_manager.cancel_transfer(&running_child.id).await;
    wait_for_batch_state(&transfer_manager, &running_cancel_batch, &["Cancelled", "CompletedWithErrors", "Failed"]).await?;
    assert_no_remote_temp_parts(&manager, &running_cancel_target).await?;

    let disconnect_target = join_remote(&remote_root, "disconnect mid transfer");
    ensure_remote_dir(&manager, &disconnect_target).await?;
    let disconnect_batch_snapshot = transfer_manager
        .queue_upload(QueueUploadRequest {
            entries: vec![selection_item(local_src.join("large.bin"), "file")],
            remote_directory: disconnect_target.clone(),
        })
        .await?;
    let disconnect_batch = latest_batch_id(&disconnect_batch_snapshot)?;
    let _running = wait_for_child_state(&transfer_manager, &disconnect_batch, &["Running", "Checking"]).await?;
    manager.disconnect().await?;
    wait_for_batch_state(&transfer_manager, &disconnect_batch, &["PausedDisconnected", "CompletedWithErrors", "Failed"]).await?;
    let reconnected = connect_and_trust(&manager, password_request()).await?;
    assert_eq!(reconnected.session.connection_state, "Connected");
    transfer_manager.retry_transfer(&disconnect_batch).await;
    wait_for_batch_state(&transfer_manager, &disconnect_batch, &["Succeeded", "CompletedWithErrors"]).await?;
    assert_remote_hash_eq_local(&manager, &join_remote(&disconnect_target, "large.bin"), &local_src.join("large.bin")).await?;
    assert_no_remote_temp_parts(&manager, &disconnect_target).await?;

    let download_disconnect_dir = local_downloads.join("disconnect-download");
    fs::create_dir_all(&download_disconnect_dir).await?;
    let remote_large = join_remote(&remote_root, "remote-large.bin");
    create_remote_large_file(&manager, &remote_large, 32 * 1024 * 1024).await?;
    let download_disconnect = transfer_manager
        .queue_download(QueueDownloadRequest {
            entries: vec![TransferSelectionItem {
                path: remote_large.clone(),
                name: "remote-large.bin".into(),
                kind: "file".into(),
            }],
            local_directory: download_disconnect_dir.display().to_string(),
        })
        .await?;
    let download_disconnect_batch = latest_batch_id(&download_disconnect)?;
    let _running_download = wait_for_child_state(&transfer_manager, &download_disconnect_batch, &["Running", "Checking"]).await?;
    manager.disconnect().await?;
    wait_for_batch_state(&transfer_manager, &download_disconnect_batch, &["PausedDisconnected", "CompletedWithErrors", "Failed"]).await?;
    connect_and_trust(&manager, password_request()).await?;
    transfer_manager.retry_transfer(&download_disconnect_batch).await;
    wait_for_batch_state(&transfer_manager, &download_disconnect_batch, &["Succeeded", "CompletedWithErrors"]).await?;
    assert_local_hash_eq_remote(&manager, &download_disconnect_dir.join("remote-large.bin"), &remote_large).await?;
    assert_no_local_temp_parts(&download_disconnect_dir).await?;

    println!("validated real-host categories: trust/auth, browse, mutate, single transfer, batch transfer, recursive transfer, conflicts, cancel, retry, disconnect/pause");

    if capabilities.remote_mode_supported {
        let _ = set_remote_mode(&manager, &join_remote(&remote_root, "unreadable-dir"), 0o040755).await;
        let _ = set_remote_mode(&manager, &join_remote(&remote_root, "unwritable-dir"), 0o040755).await;
        let _ = set_remote_mode(&manager, &protected_target, 0o040755).await;
    }
    restore_authorized_keys(&manager, &authorized_keys_path, original_authorized_keys).await?;
    cleanup_remote_root(&manager, &remote_root).await?;
    manager.disconnect().await?;
    Ok(())
}

fn password_request() -> ConnectRequest {
    ConnectRequest {
        host: HOST.into(),
        port: PORT,
        username: USERNAME.into(),
        auth: ConnectAuth::Password {
            password: PASSWORD.into(),
        },
    }
}

async fn connect_and_trust<R: Runtime>(manager: &Arc<SessionManager<R>>, request: ConnectRequest) -> Result<crate::models::RemoteConnectionSnapshot> {
    let snapshot = manager.connect(request).await?;
    if snapshot.trust_prompt.is_some() {
        return manager.resolve_trust(TrustDecision { trust: true }).await.map_err(Into::into);
    }
    Ok(snapshot)
}

fn generate_test_keypair(private_key_path: &Path) -> Result<()> {
    let status = Command::new("ssh-keygen")
        .args([
            "-q",
            "-t",
            "ed25519",
            "-N",
            KEY_PASSPHRASE,
            "-f",
            private_key_path.to_str().ok_or_else(|| anyhow!("invalid private key path"))?,
        ])
        .status()
        .context("failed to run ssh-keygen")?;
    if !status.success() {
        bail!("ssh-keygen exited with {status}");
    }
    Ok(())
}

async fn append_authorized_key<R: Runtime>(
    manager: &Arc<SessionManager<R>>,
    remote_home: &str,
    authorized_keys_path: &str,
    public_key: &str,
) -> Result<Option<Vec<u8>>> {
    let sftp = manager.open_transfer_sftp().await?;
    ensure_remote_dir(manager, &join_remote(remote_home, ".ssh")).await?;
    let existing = read_remote_file_optional_with_sftp(&sftp, authorized_keys_path).await?;
    let mut next = existing.clone().unwrap_or_default();
    if !next.is_empty() && !next.ends_with(b"\n") {
        next.push(b'\n');
    }
    next.extend_from_slice(public_key.trim().as_bytes());
    next.push(b'\n');
    write_remote_bytes_with_sftp(&sftp, authorized_keys_path, &next).await?;
    let mut metadata = sftp.symlink_metadata(authorized_keys_path).await?;
    metadata.permissions = Some(0o100600);
    sftp.set_metadata(authorized_keys_path, metadata).await?;
    let _ = sftp.close().await;
    Ok(existing)
}

async fn restore_authorized_keys<R: Runtime>(
    manager: &Arc<SessionManager<R>>,
    authorized_keys_path: &str,
    original: Option<Vec<u8>>,
) -> Result<()> {
    let sftp = manager.open_transfer_sftp().await?;
    match original {
        Some(content) => write_remote_bytes_with_sftp(&sftp, authorized_keys_path, &content).await?,
        None => {
            let _ = sftp.remove_file(authorized_keys_path).await;
        }
    }
    let _ = sftp.close().await;
    Ok(())
}

async fn build_local_fixtures(local_src: &Path) -> Result<()> {
    write_local_file(&local_src.join("alpha.txt"), b"alpha-data\n").await?;
    write_local_file(&local_src.join("space name.txt"), b"space-data\n").await?;
    write_local_file(&local_src.join("unicod\u{e9}.txt"), "unicod\u{e9}\n".as_bytes()).await?;
    write_local_file(&local_src.join("zero.bin"), b"").await?;
    write_local_file(&local_src.join(".hidden-local"), b"hidden\n").await?;
    write_local_file(&local_src.join("very-long-file-name-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.txt"), b"long\n").await?;
    create_large_local_file(&local_src.join("large.bin"), 64 * 1024 * 1024).await?;
    write_local_file(&local_src.join("dir upload/nested/deeper/file.txt"), b"nested-local\n").await?;
    write_local_file(&local_src.join("dir upload/empty/.keep"), b"").await?;
    for index in 0..80_u32 {
        let path = local_src.join(format!("dir upload/many/small-{index:03}.txt"));
        write_local_file(&path, format!("small-{index}\n").as_bytes()).await?;
    }
    Ok(())
}

async fn build_remote_fixtures<R: Runtime>(manager: &Arc<SessionManager<R>>, remote_root: &str) -> Result<FixtureCapabilities> {
    ensure_remote_dir(manager, remote_root).await?;
    create_remote_file(manager, &join_remote(remote_root, "remote-one.txt"), b"remote-one\n").await?;
    create_remote_file(manager, &join_remote(remote_root, "remote zero.bin"), b"").await?;
    create_remote_file(manager, &join_remote(remote_root, "remote unicod\u{e9}.txt"), "remote-unicod\u{e9}\n".as_bytes()).await?;
    create_remote_file(manager, &join_remote(remote_root, ".hidden-remote"), b"hidden\n").await?;
    create_remote_file(manager, &join_remote(remote_root, "nested/inner/file.txt"), b"remote nested\n").await?;
    ensure_remote_dir(manager, &join_remote(remote_root, "rename-dir")).await?;
    ensure_remote_dir(manager, &join_remote(remote_root, "download dir/nested/deeper")).await?;
    create_remote_file(manager, &join_remote(remote_root, "download dir/nested/deeper/file.txt"), b"download nested\n").await?;
    create_remote_file(manager, &join_remote(remote_root, "download dir/zero.bin"), b"").await?;
    ensure_remote_dir(manager, &join_remote(remote_root, "unreadable-dir")).await?;
    ensure_remote_dir(manager, &join_remote(remote_root, "unwritable-dir")).await?;
    let unreadable_mode_supported = try_set_remote_mode(manager, &join_remote(remote_root, "unreadable-dir"), 0o040000).await?;
    let unwritable_mode_supported = try_set_remote_mode(manager, &join_remote(remote_root, "unwritable-dir"), 0o040555).await?;
    create_remote_file(manager, &join_remote(remote_root, "symlink-target.txt"), b"symlink target\n").await?;
    let sftp = manager.open_transfer_sftp().await?;
    let remote_symlink_supported = sftp
        .symlink(join_remote(remote_root, "symbolic-link"), join_remote(remote_root, "symlink-target.txt"))
        .await
        .is_ok();
    let _ = sftp.close().await;
    Ok(FixtureCapabilities {
        remote_mode_supported: unreadable_mode_supported && unwritable_mode_supported,
        remote_symlink_supported,
    })
}

async fn ensure_remote_dir<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str) -> Result<()> {
    let sftp = manager.open_transfer_sftp().await?;
    let mut current = String::from("/");
    for segment in path.split('/').filter(|segment| !segment.is_empty()) {
        current = if current == "/" {
            format!("/{segment}")
        } else {
            format!("{current}/{segment}")
        };
        if let Err(error) = sftp.create_dir(&current).await {
            if !remote_exists_with_sftp(&sftp, &current).await? {
                let _ = sftp.close().await;
                return Err(anyhow!(error));
            }
        }
    }
    let _ = sftp.close().await;
    Ok(())
}

async fn create_remote_file<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str, content: &[u8]) -> Result<()> {
    let parent = parent_remote(path);
    ensure_remote_dir(manager, &parent).await?;
    let sftp = manager.open_transfer_sftp().await?;
    write_remote_bytes_with_sftp(&sftp, path, content).await?;
    let _ = sftp.close().await;
    Ok(())
}

async fn create_remote_large_file<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str, size: usize) -> Result<()> {
    let parent = parent_remote(path);
    ensure_remote_dir(manager, &parent).await?;
    let sftp = manager.open_transfer_sftp().await?;
    let mut file = sftp.create(path).await?;
    let block = vec![b'R'; 256 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let take = remaining.min(block.len());
        file.write_all(&block[..take]).await?;
        remaining -= take;
    }
    file.sync_all().await?;
    file.shutdown().await?;
    let _ = sftp.close().await;
    Ok(())
}

async fn write_remote_bytes_with_sftp(sftp: &SftpSession, path: &str, content: &[u8]) -> Result<()> {
    let mut file = sftp.create(path).await?;
    file.write_all(content).await?;
    file.sync_all().await?;
    file.shutdown().await?;
    Ok(())
}

async fn read_remote_file<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str) -> Result<Vec<u8>> {
    let sftp = manager.open_transfer_sftp().await?;
    let content = read_remote_file_with_sftp(&sftp, path).await?;
    let _ = sftp.close().await;
    Ok(content)
}

async fn read_remote_file_optional_with_sftp(sftp: &SftpSession, path: &str) -> Result<Option<Vec<u8>>> {
    match sftp.open(path).await {
        Ok(mut file) => {
            let mut content = Vec::new();
            file.read_to_end(&mut content).await?;
            Ok(Some(content))
        }
        Err(SftpError::Status(status)) if matches!(status.status_code, russh_sftp::protocol::StatusCode::NoSuchFile) => Ok(None),
        Err(error) => Err(anyhow!(error)),
    }
}

async fn read_remote_file_with_sftp(sftp: &SftpSession, path: &str) -> Result<Vec<u8>> {
    let mut file = sftp.open(path).await?;
    let mut content = Vec::new();
    file.read_to_end(&mut content).await?;
    Ok(content)
}

async fn set_remote_mode<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str, mode: u32) -> Result<()> {
    let sftp = manager.open_transfer_sftp().await?;
    let mut metadata = sftp.symlink_metadata(path).await?;
    metadata.permissions = Some(mode);
    sftp.set_metadata(path, metadata).await?;
    let _ = sftp.close().await;
    Ok(())
}

async fn try_set_remote_mode<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str, mode: u32) -> Result<bool> {
    match set_remote_mode(manager, path, mode).await {
        Ok(()) => Ok(true),
        Err(error) => {
            println!("remote chmod skipped for {path}: {error}");
            Ok(false)
        }
    }
}

async fn cleanup_remote_root<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str) -> Result<()> {
    if let Ok(sftp) = manager.open_transfer_sftp().await {
        let _ = remove_remote_tree(&sftp, path).await;
        let _ = sftp.close().await;
    }
    Ok(())
}

async fn remove_remote_tree(sftp: &SftpSession, path: &str) -> Result<()> {
    let mut stack = vec![(path.to_string(), false)];
    while let Some((current, visited)) = stack.pop() {
        match sftp.symlink_metadata(&current).await {
            Ok(metadata) if metadata.is_dir() => {
                if visited {
                    let _ = sftp.remove_dir(&current).await;
                    continue;
                }

                stack.push((current.clone(), true));
                let entries = sftp.read_dir(&current).await?.collect::<Vec<_>>();
                for entry in entries {
                    let child = join_remote(&current, &entry.file_name());
                    match entry.file_type() {
                        russh_sftp::protocol::FileType::Dir => stack.push((child, false)),
                        _ => {
                            let _ = sftp.remove_file(&child).await;
                        }
                    }
                }
            }
            Ok(_) => {
                let _ = sftp.remove_file(&current).await;
            }
            Err(_) => {}
        }
    }
    Ok(())
}

async fn wait_for_batch_state<R: Runtime>(transfer_manager: &Arc<TransferManager<R>>, batch_id: &str, states: &[&str]) -> Result<TransferJob> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut last_seen = None;
    loop {
        let jobs: Vec<TransferJob> = transfer_manager.debug_all_jobs().await;
        if let Some(job) = jobs.into_iter().find(|job| job.id == batch_id) {
            last_seen = Some((job.state.clone(), job.error_message.clone()));
            if states.contains(&job.state.as_str()) {
                return Ok(job);
            }
        }
        if Instant::now() > deadline {
            bail!("timed out waiting for batch {batch_id} in states {:?}; last seen {:?}", states, last_seen);
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_conflict_child<R: Runtime>(transfer_manager: &Arc<TransferManager<R>>, batch_id: &str) -> Result<TransferJob> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let jobs: Vec<TransferJob> = transfer_manager.debug_all_jobs().await;
        if let Some(job) = jobs.into_iter().find(|job| {
            job.batch_id.as_deref() == Some(batch_id)
                && job.kind == "child"
                && job.state == "AwaitingConflictDecision"
        }) {
            return Ok(job);
        }
        if Instant::now() > deadline {
            bail!("timed out waiting for conflict child in batch {batch_id}");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_child_state<R: Runtime>(transfer_manager: &Arc<TransferManager<R>>, batch_id: &str, states: &[&str]) -> Result<TransferJob> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let jobs: Vec<TransferJob> = transfer_manager.debug_all_jobs().await;
        if let Some(job) = jobs.into_iter().find(|job| {
            job.batch_id.as_deref() == Some(batch_id) && job.kind == "child" && states.contains(&job.state.as_str())
        }) {
            return Ok(job);
        }
        if Instant::now() > deadline {
            bail!("timed out waiting for child state {:?} in batch {batch_id}", states);
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_hidden_child_state<R: Runtime>(
    transfer_manager: &Arc<TransferManager<R>>,
    batch_id: &str,
    state: &str,
    name: &str,
) -> Result<TransferJob> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let jobs: Vec<TransferJob> = transfer_manager.debug_all_jobs().await;
        if let Some(job) = jobs.into_iter().find(|job| {
            job.batch_id.as_deref() == Some(batch_id) && job.kind == "child" && job.state == state && job.name == name
        }) {
            return Ok(job);
        }
        if Instant::now() > deadline {
            bail!("timed out waiting for hidden child {name} in state {state}");
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn latest_failed_child<R: Runtime>(transfer_manager: &Arc<TransferManager<R>>, batch_id: &str) -> Result<TransferJob> {
    let jobs: Vec<TransferJob> = transfer_manager.debug_all_jobs().await;
    jobs.into_iter()
        .find(|job| job.batch_id.as_deref() == Some(batch_id) && job.kind == "child" && job.state == "Failed")
        .ok_or_else(|| anyhow!("failed child not found for batch {batch_id}"))
}

async fn visible_rows_for_batch<R: Runtime>(transfer_manager: &Arc<TransferManager<R>>, batch_id: &str) -> usize {
    transfer_manager
        .snapshot()
        .await
        .jobs
        .into_iter()
        .filter(|job| job.id == batch_id || job.batch_id.as_deref() == Some(batch_id))
        .count()
}

fn latest_batch_id(snapshot: &crate::models::TransferQueueSnapshot) -> Result<String> {
    snapshot
        .jobs
        .iter()
        .rev()
        .find(|job| job.kind == "batch")
        .map(|job| job.id.clone())
        .ok_or_else(|| anyhow!("no batch job found in snapshot"))
}

fn selection_item(path: PathBuf, kind: &str) -> TransferSelectionItem {
    TransferSelectionItem {
        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        path: path.display().to_string(),
        kind: kind.into(),
    }
}

async fn assert_remote_hash_eq_local<R: Runtime>(manager: &Arc<SessionManager<R>>, remote_path: &str, local_path: &Path) -> Result<()> {
    let remote = read_remote_file(manager, remote_path).await?;
    let local = fs::read(local_path).await?;
    assert_eq!(sha256_bytes(&remote), sha256_bytes(&local), "hash mismatch for {remote_path} vs {}", local_path.display());
    Ok(())
}

async fn assert_local_hash_eq_remote<R: Runtime>(manager: &Arc<SessionManager<R>>, local_path: &Path, remote_path: &str) -> Result<()> {
    assert_remote_hash_eq_local(manager, remote_path, local_path).await
}

async fn assert_remote_tree_matches_local<R: Runtime>(
    local_root: &Path,
    manager: &Arc<SessionManager<R>>,
    remote_root: &str,
) -> Result<()> {
    let local_hashes = hash_local_tree(local_root).await?;
    let remote_hashes = hash_remote_tree(manager, remote_root).await?;
    assert_eq!(local_hashes, remote_hashes, "tree hash mismatch");
    Ok(())
}

async fn hash_local_tree(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut read_dir = fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                let rel = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
                result.insert(rel, sha256_bytes(&fs::read(&path).await?));
            }
        }
    }
    Ok(result)
}

async fn hash_remote_tree<R: Runtime>(manager: &Arc<SessionManager<R>>, root: &str) -> Result<BTreeMap<String, String>> {
    let sftp = manager.open_transfer_sftp().await?;
    let mut result = BTreeMap::new();
    let mut stack = vec![root.to_string()];
    while let Some(dir) = stack.pop() {
        let entries = sftp.read_dir(&dir).await?.collect::<Vec<_>>();
        for entry in entries {
            let path = join_remote(&dir, &entry.file_name());
            match entry.file_type() {
                russh_sftp::protocol::FileType::Dir => stack.push(path),
                russh_sftp::protocol::FileType::File => {
                    let rel = path.trim_start_matches(root).trim_start_matches('/').to_string();
                    result.insert(rel, sha256_bytes(&read_remote_file_with_sftp(&sftp, &path).await?));
                }
                _ => {}
            }
        }
    }
    let _ = sftp.close().await;
    Ok(result)
}

async fn remote_file_exists<R: Runtime>(manager: &Arc<SessionManager<R>>, path: &str) -> Result<bool> {
    let sftp = manager.open_transfer_sftp().await?;
    let exists = remote_exists_with_sftp(&sftp, path).await?;
    let _ = sftp.close().await;
    Ok(exists)
}

async fn remote_exists_with_sftp(sftp: &SftpSession, path: &str) -> Result<bool> {
    match sftp.symlink_metadata(path).await {
        Ok(_) => Ok(true),
        Err(SftpError::Status(status)) if matches!(status.status_code, russh_sftp::protocol::StatusCode::NoSuchFile) => Ok(false),
        Err(error) => Err(anyhow!(error)),
    }
}

async fn assert_no_remote_temp_parts<R: Runtime>(manager: &Arc<SessionManager<R>>, dir: &str) -> Result<()> {
    let sftp = manager.open_transfer_sftp().await?;
    let names = sftp.read_dir(dir).await?.map(|entry| entry.file_name()).collect::<BTreeSet<_>>();
    assert!(names.iter().all(|name| !name.contains("warp-part")), "remote temp artifacts remain in {dir}: {names:?}");
    let _ = sftp.close().await;
    Ok(())
}

async fn assert_no_local_temp_parts(dir: &Path) -> Result<()> {
    let mut read_dir = fs::read_dir(dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(!name.contains("warp-part"), "local temp artifact remains: {name}");
    }
    Ok(())
}

fn names(pane: &crate::models::PaneSnapshot) -> BTreeSet<String> {
    pane.entries.iter().map(|entry| entry.name.clone()).collect()
}

fn join_remote(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", parent.trim_end_matches('/'), name)
    }
}

fn parent_remote(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) | None => "/".into(),
        Some((parent, _)) => parent.into(),
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn set_local_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

async fn write_local_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(path, bytes).await?;
    Ok(())
}

async fn create_large_local_file(path: &Path, size: usize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut file = fs::File::create(path).await?;
    let block = vec![b'L'; 256 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let take = remaining.min(block.len());
        file.write_all(&block[..take]).await?;
        remaining -= take;
    }
    file.sync_all().await?;
    Ok(())
}
