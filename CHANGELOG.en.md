# Changelog

All notable changes to `ii` are documented here. The default Chinese version is [CHANGELOG.md](CHANGELOG.md).

## 0.3.7 - 2026-08-07

### Added

- Added the `ii http` read-only directory site, `ii paste` text sharing, `ii drop` resumable upload drop box, `ii pac` PAC hosting, and `ii speed` bidirectional HTTP chunked throughput tests. They reuse LAN URLs, QR codes, tokens, and discovery.
- Added the `ii proxy` HTTP forward proxy, `ii tcp`/`ii udp` fixed-target forwarders, `ii ping` TCP connect latency probe, `ii port` concurrent port checker, `ii health` HTTP(S)/TCP health check, and `ii wake` Wake-on-LAN sender.

## 0.3.6 - 2026-08-07

### Added

- `ii recv --trace` now reports Iroh's selected LAN, direct, or relay path and RTT.
- `ii send`, `ii watch`, and `ii queue` accept repeated `--relay`; multiple explicit relays are probed for reachability and latency, while default n0 relay behavior is unchanged.
- `ii send --web` and `ii web --upload` support resumable browser uploads; reselecting the same file after a disconnect or refresh continues it.
- Added `ii socks5`, an ordinary SOCKS5 proxy with `CONNECT`, `UDP ASSOCIATE`, `BIND`, IPv4, IPv6, domain targets, and optional username/password authentication.

## 0.3.5 - 2026-08-07

### Added

- `ii send` and `ii recv` support `--checksum md5|sha256`; checksums are calculated and printed locally only, never stored in tickets or compared automatically.
- `ii send <file> --preserve-metadata` uses the existing tar payload to retain single-file mtime, permissions, and read-only metadata; directory and multi-path archives support `--symlinks follow|preserve|reject`.
- Added process-local FIFO `ii queue` and polling `ii watch`, including delay, repeat, stability detection, and existing sending backends.
- `ii send`, `ii recv`, `ii watch`, and `ii queue` support `--quic-port` to fix the P2P Iroh UDP port; `ii doctor --nat` reports a short-lived UDP, NAT, and relay probe.
- `ii web --once` exits after the first complete ordinary-file download; directory pages, HEAD, Range, uploads, and failed requests do not stop it.

## 0.3.4 - 2026-08-06

### Changed

- `ii send --web` now exits after its first completed download by default; add `-t` to keep serving. Page visits, uploads, and failed downloads do not end the service.

## 0.3.3 - 2026-08-05

### Added

- `ii dav` now supports HTTP Basic Auth; `--username` and `--password` must be supplied together.
- `ii dav` now supports HTTPS: `--tls` generates a temporary self-signed certificate, while `--domain`, `--cert`, and `--key` support custom DNS names and PEM certificates.

## 0.3.2 - 2026-08-05

### Added

- Added `install.sh` for Linux x86_64 and Apple Silicon macOS. It downloads the latest Release, verifies SHA-256, and installs to `~/.local/bin` or `II_INSTALL_DIR`.

### Changed

- Release assets now include `checksums.txt`, and Linux CI verifies the installed version through the installer.

## 0.3.1 - 2026-08-05

### Added

- Added `ii send --r2` with separate Cloudflare R2 profiles, and `ii send --azure` for Azure Block Blob with Shared Key or Container SAS authentication.

### Changed

- `ii send --s3` now means generic S3-compatible object storage only, and first-run setup collects endpoint, region, bucket, credentials, and path-style mode.
- Legacy `provider = "cloudflare-r2"` entries under `[storage.s3.<name>]` are no longer migrated; move them to `[storage.r2.<name>]` and use `--r2`. Existing signed object URL tickets remain receivable.

## 0.3.0 - 2026-08-05

### Added

- `ii send` now supports mixed file/folder sends, `--include`/`--exclude` filters, a global `--rate` cap, and JSON Lines events with `--json`.
- Added `ii discover` LAN discovery and `--bind` IPv4/IPv6 listeners for `ii web`, `ii dav`, and `ii send --web`.
- Added a read-write `ii dav` LAN WebDAV service with desktop-client methods, Range, chunked PUT, and process-local locks.

## 0.2.9 - 2026-08-04

### Changed

- `ii send -t` now runs up to 16 concurrent receive tasks with up to 1,000 FIFO queued connections; one disconnected or timed-out receiver no longer blocks a resumed transfer in another free slot.

## 0.2.8 - 2026-08-03

### Changed

- `ii relay` now starts an HTTP relay without arguments on a random port and supports `--port` for a fixed port; it prints reachable IPv4 URLs and other interface URLs.
- `--tls` is now an optional self-signed HTTPS switch, `--domain` selects the TLS name, and `--cert` plus `--key` replace the generated certificate; `--public` and `-H` were removed.
- `ii send --relay` and `ii tunnel -s --relay` accept HTTP or HTTPS URLs; `-k` is HTTPS-only.

## 0.2.7 - 2026-08-03

### Changed

- Web uploads for `ii send ... --web` and `ii web` are disabled by default; only `--upload` renders controls and opens the upload endpoint, while `--path` without it is ignored.
- Bare `--token` for `ii send ... --web`, `ii web`, and `ii webrtc` now generates and prints a 32-character path access token; `--token <value>` and `--token=<value>` remain supported.

## 0.2.6 - 2026-08-03

### Added

- `ii send ... --web`, `ii web`, and `ii webrtc` support `--port <port>` to select the HTTP listener port; omitting it keeps random port selection.
- Added `ii help [command]` for root or command-specific help.

## 0.2.5 - 2026-08-01

### Fixed

- Fixed the missing `PathBuf` import when compiling tests on Unix platforms.

## 0.2.4 - 2026-08-01

### Changed

- Split the CLI core into focused command, service, transport, backend, web, ticket, storage, and relay modules while preserving CLI behavior, the GUI facade, ticket encoding, and network protocols.

## 0.2.3 - 2026-08-01

### Added

- `ii webrtc` now sends text to the selected device; long text is split into UTF-8 byte chunks no larger than 1 MiB, reassembled as one received message, and can be copied without storing chat history.

## 0.2.2 - 2026-08-01

### Added

- Added `ii tunnel -s <target-host:port>` and `ii tunnel -c <ticket>` for temporary TCP forwarding over existing Iroh direct or relay paths; tickets carry an access key and any explicit relay TLS trust policy.

## 0.2.1 - 2026-07-31

### Added

- Added HTTP single-range, `HEAD`, and common media/PDF/image MIME responses for normal `ii web` files, enabling native browser playback and resumable downloads.

## 0.2.0 - 2026-07-31

### Added

- Added `ii webrtc [--token <value>]` for temporary LAN browser-to-browser WebRTC file transfer rooms; it prints a QR code and all IPv4 LAN URLs, keeps file bytes out of the `ii` process, and uses no public STUN/TURN.

### Fixed

- Fixed `ii webrtc` DataChannel setup between mobile browsers when mDNS host candidates could not be resolved or ICE gathering did not finish; signalling now uses the client's LAN IPv4 and trickles candidates immediately.
- `ii webrtc` now verifies an ICE host candidate before joining; browsers with disabled or blocked WebRTC receive an explicit message instead of only discovering peers without being able to transfer.

### Documentation

- Added [webrtc.md](webrtc.md) covering LAN scope, browser requirements, memory limits, the path token, and unsupported capabilities.

## 0.1.19 - 2026-07-31

### Added

- Added `ii web [directory]` for temporary LAN recursive directory browsing, normal file responses, and multi-file uploads; it supports `--token` and `--path`, and prints a QR code plus all IPv4 LAN URLs in the terminal.
- Added `ii send ... --web --path <dir>` to write web uploads directly into a chosen directory; relative paths are based on the startup directory, while the default remains `./ii/`.

## 0.1.18 - 2026-07-30

### Added

- Added multi-file uploads to the `--web` page, streaming files into `./ii/` under the startup directory and overwriting same-name files.
- Added the `ii send ... --web --token <value>` path access token; page, download, upload, terminal URLs, and QR codes use the path, while missing or incorrect paths return `404`.

### Documentation

- Updated the Chinese and English READMEs and command manual for web uploads and `--token` usage and limits.

## 0.1.17 - 2026-07-30

### Added

- Added `ii send <file-or-folder> --web` for a temporary LAN HTTP sharing page; terminal and page QR codes are included, and folders download as `.tar` archives.
- Print the primary LAN URL and remaining physical and virtual adapter IPv4 URLs, with a responsive phone layout.

## 0.1.16 - 2026-07-30

### Fixed

- Fixed platform-specific UPX release handling: clear a stale Windows extraction directory, and pass `--force-macos` when compressing macOS Mach-O binaries.

## 0.1.15 - 2026-07-30

### Changed

- The release workflow continues to report UPX-compressed CLI sizes for all targets, without blocking publication on a size limit.

## 0.1.14 - 2026-07-30

### Changed

- Restricted the release workflow to the three CLI artifacts; fixed Linux and macOS UPX path handling, which had overwritten UPX's reserved environment variable.

## 0.1.13 - 2026-07-30

### Added

- Added `ii send --ftp` and `ii send --sftp` for FTP and SFTP transfer backends supporting files, stdin, and folders.
- Added FTP/SFTP profiles, portable tickets, receiver-side remote-object deletion, and `ii doctor` configuration checks.
- Added the Slint-based `ii-gui` desktop client with sending, receiving, S3/WebDAV/TLS relay profile management, a transfer queue, and diagnostics.

### Changed

- Added Windows GUI executables, Linux AppImages, and macOS `.app.zip` artifacts to the release workflow.
- Pruned relay, S3, WebDAV, FTP, and logging dependencies while retaining current CLI, configuration, and transfer-protocol compatibility; added UPX integrity checks and a 1 MiB CLI size gate for all release targets.

### Documentation

- Added FTP and SFTP backend guides and synchronized the English README and full command manual.

## 0.1.12 - 2026-07-17

### Changed

- Updated release version metadata.

## 0.1.11 - 2026-07-17

### Added

- Added `ii relay --public <https-url>` to generate and persist a self-signed HTTPS relay certificate.
- Added `ii send --relay <https-url> -k` to trust a self-signed relay and carry that policy in the ticket for receivers.

### Changed

- Made explicit `--relay` sends and receives relay-only, without UDP, LAN discovery, or direct paths.
- Kept normal system TLS verification for manual TLS relays; first use of a self-signed relay can still be replaced by a man-in-the-middle.

### Documentation

- Updated self-signed relay, manual TLS, port, state-file, and security-boundary guidance.

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
