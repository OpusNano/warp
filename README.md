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

This repository currently contains the desktop shell, a Rust-backed local filesystem pane with navigation, filtering, inline rename, and in-app delete confirmation, plus a real SSH/SFTP browsing and transfer slice with:

- explicit host trust verification with a first-seen host prompt and known-host mismatch blocking
- password or private-key authentication
- connect, disconnect, and reconnect flows in the existing split-pane shell
- real remote directory listing, enter-directory, go-up, and refresh in the right pane
- single-file SFTP upload and download through the existing queue panel
- Rust-owned transfer queue state with progress, cancel, success, failure, overwrite conflict handling, and clearable completed history
- a compact transfer log panel with newest jobs first and internal scrolling so the split-pane browser keeps its height
- SSH keepalive-driven session liveness handling so stale remote sessions fall back to a clear disconnected state

Still intentionally out of scope in the current slice:

- SCP work beyond future compatibility boundaries
- saved connections
- remote rename/delete/create-directory actions
- recursive directory transfers, multi-select transfer batches, and drag-and-drop

## Current transfer behavior

- Transfers are single-file SFTP uploads or downloads only.
- Newer transfer jobs appear at the top of the queue/history panel.
- Overwrite conflicts are resolved inline from the queue panel.
- Completed transfer history can be cleared without affecting queued or running jobs.
- If the SSH session drops, the remote side returns to a disconnected state instead of staying silently stale.
