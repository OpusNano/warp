# AGENTS

Repo-specific guidance for contributors and coding agents working on `warp`.

## Product boundaries

- `warp` is an SFTP-first desktop client.
- SCP is compatibility-only and transfer-only.
- Do not add FTP, FTPS, WebDAV, cloud integrations, sync engines, terminal emulation, plugins, or team features unless explicitly requested.

## Technical direction

- Desktop shell: Tauri 2.x
- Backend: Rust
- Frontend: SolidJS + Vite + TypeScript
- Styling: Tailwind CSS
- Protocol foundation: `russh` + `russh-sftp`
- Do not introduce React.
- Do not add a generic remote filesystem abstraction in v1.

## UX constraints

- The split-pane workflow defines MVP completeness.
- Keep the UI minimal, sharp, and desktop-like.
- Preserve the existing dark visual system unless a task explicitly changes it.
- Avoid adding explanatory marketing copy to the UI.

## Implementation rules

- Keep business logic in Rust and keep the frontend thin.
- Do not wire buttons or backend functionality unless the task requires it.
- Prefer focused changes over broad refactors.
- Do not add new abstractions without a concrete need.
- The current remote slice covers connection, trust verification, authentication, remote browsing, basic remote mutation actions (create directory, rename, delete with recursive confirmation when needed, plus multi-select delete), and SFTP upload/download with a Rust-owned batch queue.
- The transfer queue is a compact log panel: newest jobs first, batch-oriented rows, inline conflict actions with explicit source/destination context, clearable completed history, and no layout behavior that pushes the split panes upward.
- Session liveness is explicit: keepalive and disconnect handling should converge on a clear disconnected state rather than leaving a stale "connected" UI.
- Do not imply SCP, saved connections, broader remote mutation actions, or other advanced filesystem workflows work unless a task explicitly implements them.

## Validation

- Frontend typecheck: `npm run check`
- Frontend build: `npm run build`
- Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- Real-host transfer/session validation also runs from `cargo test --manifest-path src-tauri/Cargo.toml`
- AppImage build: `npm run tauri build -- --bundles appimage`
