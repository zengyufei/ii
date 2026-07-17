# Changelog

All notable changes to `ii` are documented here. The default Chinese version is [CHANGELOG.md](CHANGELOG.md).

## Unreleased

Nothing yet.

## 0.1.10 - 2026-07-17

### Changed

- Added `ii relay --tls <domain> --cert <path> --key <path>` for HTTPS relays using operator-provided PEM certificate and key files.
- TLS mode no longer exposes a public HTTP relay; certificate files and the domain are owned by the operator.
- Removed ACME certificate issuance, certificate renewal, and QUIC address discovery while retaining the HTTP-only default relay.
- Made `ii doctor` check `3340/tcp` by default.

### Documentation

- Updated manual-certificate HTTPS and relay port guidance.

## 0.1.9 - 2026-07-17

### Added

- Made `ii relay` start an HTTP-only relay on `3340/tcp` without requiring a domain or certificate.

### Changed

- Made TLS, QUIC address discovery, and metrics opt-in through the relay configuration.
- Removed default DNS peer discovery and unused CLI dependencies to reduce the release dependency tree.

### Documentation

- Updated relay startup, HTTPS/QUIC configuration, and reverse-proxy guidance.

## 0.1.8 - 2026-07-16

### Fixed

- Fixed the Windows config path unit test so it passes on Linux/macOS runners without relying on backslash path parsing.

## 0.1.7 - 2026-07-16

### Changed

- Enabled release LTO, strip, `opt-level = "z"`, and `panic = "abort"` to further reduce binary size.
- Made `ii doctor` report metrics as disabled when the `relay-metrics` feature is not enabled.

### Fixed

- Fixed S3/WebDAV default profile selection so the old shared `[storage].profile` field no longer crosses backend boundaries.
- Kept compatibility migration from the old `[storage.s3.cloudflare]` profile while standardizing the default S3 profile on `default`.

## 0.1.6 - 2026-07-16

### Documentation

- Added an `ii send --s3` S3/R2 transfer example to the advanced README usage section.

## 0.1.5 - 2026-07-16

### Added

- Added `ii send --webdav` for sending files, stdin, and folders through a WebDAV transfer backend.
- Added `ii send --webdav -p` to create portable tickets containing the WebDAV URL, username, and password for receivers without local config.
- Added `ii send --webdav -d` so the receiver can try deleting the remote WebDAV object after a successful receive.
- Added `ii send --profile <name>` for selecting an S3 or WebDAV backend profile.
- Added WebDAV config checks to `ii doctor`.

## 0.1.4 - 2026-07-16

### Changed

- Changed Windows Release compression to use the bundled UPX 5.1.0 binary from the repository instead of downloading UPX during GitHub Actions runs.

## 0.1.3 - 2026-07-16

### Added

- Added live `ii recv` transfer progress and speed display for interactive terminals.
- Added explicit `ii send -c` clipboard copy for the printed `ii recv ...` command.
- Added `ii send -o <path>` to write the printed `ii recv ...` command to a file.
- Added elapsed time and average speed to the final `ii recv` completion line.

## 0.1.2 - 2026-07-15

### Changed

- Added the official `ii` logo assets.
- Added the logo to the README header.
- Embedded `logo.ico` into the Windows executable during builds.

## 0.1.1 - 2026-07-15

### Changed

- Changed GitHub Actions Release assets to publish raw binaries instead of zip or tar.gz archives.
- Kept UPX compression for the Windows Release executable.
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
