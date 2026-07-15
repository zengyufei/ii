# Changelog

All notable changes to `ii` are documented here.

## 0.1.1 - 2026-07-15

### Changed

- Changed GitHub Actions release assets to publish raw binaries instead of zip or tar.gz archives.
- Kept UPX compression for the Windows release executable.
- Added README screenshots for the temporary coworker file transfer flow.

## 0.1.0 - 2026-07-15

### Added

- Added the `ii` CLI with `send`, `recv`, `relay`, `doctor`, and `version`.
- Added file, folder, and stdin transfer support.
- Added default one-shot `ii send`; use `-t` to keep the sender alive for multiple receivers.
- Added automatic resume, overwrite, and identical-file skip for file/stdin receives.
- Added relay management through `ii relay` with config generation and port overrides.
- Added `ii recv --trace` for connection and transfer timing diagnostics.

### Changed

- Changed directory receive behavior so a sent folder extracts as one top-level folder, not a duplicated nested folder.
- Changed receive connection strategy to fall back to relay-only after a short direct-address window.

### Fixed

- Fixed incomplete transfer handling by waiting for connection close after payload finish.
- Fixed sender timeout noise after successful receives.

### Breaking

- Removed `ii send --once`; one-shot send is now the default.
- Added `ii send -t` for the old keep-alive behavior.
