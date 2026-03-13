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

## Current status

This repository currently contains the initial application shell, visual system, Rust-first module layout, and GitHub Actions setup for Linux-first CI and AppImage releases.
