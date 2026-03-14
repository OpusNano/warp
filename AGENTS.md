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
- The current remote slice covers connection, trust verification, authentication, and remote browsing only; do not imply remote file mutation or transfers work unless a task explicitly implements them.

## Validation

- Frontend typecheck: `npm run check`
- Frontend build: `npm run build`
- Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- AppImage build: `npm run tauri build -- --bundles appimage`
