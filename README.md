# warp

`warp` is a lightweight split-pane desktop client for SFTP, built with Tauri 2, Rust, SolidJS, and Tailwind CSS.

## Product direction

- SFTP is the real product.
- SCP stays compatibility-only and transfer-only.
- `russh` + `russh-sftp` are the protocol foundation.
- The split-pane workflow defines MVP completeness.

## Stack

- Tauri 2.x
- Rust backend with `tokio`
- SolidJS + Vite + TypeScript frontend
- Tailwind CSS for styling
- Linux AppImage as the first release target

## Local development

```bash
npm ci
npm run dev
```

To run the desktop shell:

```bash
npm run tauri dev
```

## Validation

```bash
npm run check
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build -- --bundles appimage
```

## Current status

This repository currently contains the desktop shell, a Rust-backed local filesystem pane with navigation, filtering, inline rename, and in-app delete confirmation, plus a real SSH/SFTP browsing, mutation, and transfer slice with:

- explicit host trust verification with a first-seen host prompt and known-host mismatch blocking
- password or private-key authentication
- connect, disconnect, and reconnect flows in the existing split-pane shell
- real remote directory listing, enter-directory, go-up, and refresh in the right pane
- remote create-directory, inline rename, and confirmed delete for files and directories, including multi-select delete in the existing confirmation flow
- batch-aware SFTP upload and download through the existing queue panel, including multi-file selection and recursive directory transfers
- Rust-owned transfer queue state with per-file execution, batch progress, cancel, retry, success, failure, overwrite conflict handling, and clearable completed history
- a compact transfer log panel with newest jobs first and internal scrolling so the split-pane browser keeps its height
- SSH keepalive-driven session liveness handling so stale remote sessions fall back to a clear disconnected state
- real-host validation coverage for transfer, conflict, cancel, retry, and disconnect behavior in `cargo test --manifest-path src-tauri/Cargo.toml`

Still intentionally out of scope in the current slice:

- SCP work beyond future compatibility boundaries
- saved connections
- broad remote mutation flows beyond create-directory, rename, and delete
- drag-and-drop

## Current remote mutation behavior

- Remote create-directory, rename, and delete all run through Rust over SFTP.
- Remote rename and file delete follow normal Unix/SFTP server semantics; Warp does not add fake ownership restrictions on top of the server.
- Deleting a non-empty remote directory prompts for recursive removal in the existing delete confirmation flow.
- Recursive remote delete is supported only through that confirmation flow and refreshes the visible pane after success or failure.
- If recursive delete fails partway through, Warp shows a short summary instead of raw protocol noise and refreshes the pane to the server's current state.

## Current transfer behavior

- Transfers run over SFTP and can be queued as single-file, multi-file, mixed-selection, or recursive directory batches.
- Newer transfer jobs appear at the top of the queue/history panel.
- Overwrite conflicts are resolved inline from the queue panel and identify the exact conflicting source and destination item.
- Queue rows stay batch-oriented even when a single child file is blocked on conflict, cancellation, or retry.
- Batch progress is aggregated while individual file work stays Rust-owned and internal to the queue engine.
- Completed transfer history can be cleared without affecting queued or running jobs.
- If the SSH session drops, affected batches pause, the remote side returns to a disconnected state, and the batch can be retried after reconnect.
