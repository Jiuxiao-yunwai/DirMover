# DirMover repository instructions

Before changing files, check `git status --short --branch` and the current version.

- `main` contains only formally released, stable versions. Do not develop directly on `main`.
- `develop` is the integration branch for features, fixes, and performance work.
- Do not merge `develop` into `main`, create a tag, push, or publish a release unless the user explicitly requests it.
- Development versions use `<target>-dev.N`; formal releases remove the prerelease suffix.
- Keep versions synchronized in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`. Regenerate or update `src-tauri/Cargo.lock` when the Rust package version changes.
- Before merging to `main`, run the complete Rust and web quality gates documented in `docs/development.md`.
- Preserve all unrelated and uncommitted user changes.

Current development cycle:

- Stable version on `main`: `2.2.0`
- Development branch: `develop`
- Current development version: `2.2.0`
- Target release: `2.2.0`

See `docs/development.md` for the full branch, version, and release workflow.
