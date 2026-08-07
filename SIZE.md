# Binary Size Ledger

The release artifact is the UPX-compressed CLI executable. The release workflow
verifies the packed artifact and reports its size on Windows, Linux, and macOS.

## Windows x86_64

All measurements use `cargo build -p ii --release --locked` followed by UPX
`5.1.0` with `--best --lzma` and `upx -t`.

| Revision | Unpacked bytes | UPX bytes | Change | Verification |
| --- | ---: | ---: | ---: | --- |
| Before FTP/SFTP | 5,700,608 | 2,085,376 | baseline | prior controlled build |
| Before dependency pruning | 7,689,216 | 2,624,512 | +539,136 | prior controlled build |
| Regex and relay dependency pruning | 6,364,672 | 2,288,640 | -335,872 | `cargo test -p ii --locked`, `upx -t` |
| S3 client and credential-chain pruning | 6,115,840 | 2,204,160 | -84,480 | `cargo test -p ii --locked`, S3 SigV4/multipart request tests, `upx -t` |
| WebDAV client API/XML-model pruning | 5,997,568 | 2,165,248 | -38,912 | `cargo test -p ii --locked`, Basic/Digest/PROPFIND request tests, `upx -t` |
| Relay QUIC-listener feature split | 5,967,872 | 2,158,592 | -6,656 | `cargo test -p ii --locked`, `upx -t` |
| Relay management endpoint pruning | 5,965,824 | 2,157,056 | -1,536 | `cargo test -p ii --locked`, relay `/ping` and `/generate_204` smoke test, `upx -t` |
| Relay logging subscriber pruning | 5,922,816 | 2,142,208 | -14,848 | `cargo test -p ii --locked`, `upx -t` |
| WebDAV Multi-Status validation | 5,955,584 | 2,153,984 | +11,776 | `cargo test -p ii --locked`, malformed XML regression tests, `upx -t` |
| WebDAV strict XML document validation | 5,956,096 | 2,151,936 | -2,048 | `cargo test -p ii --locked` (66 tests), malformed, non-WebDAV, multiple-root, and trailing-CDATA XML regression tests, `upx -t` |
| Rejected: relay rate-limit feature split | 5,952,000 | 2,154,496 | +1,536 | Full `ii` regression suite (66 tests), full relay-library feature compile, `upx -t`; reverted because packed size grew |
| Final release reproduction | 5,956,096 | 2,153,472 | -512 | `cargo test -p ii --locked` (66 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Single-thread Tokio runtime | 5,946,368 | 2,148,864 | -4,608 | `cargo test -p ii --locked` (66 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| LAN web-share QR SVG and terminal QR | 5,970,944 | 2,161,152 | +12,288 | `cargo test -p ii --locked` (76 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Web download QR and responsive share page | 5,975,040 | 2,159,616 | -1,536 | `cargo test -p ii --locked` (76 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Web-share other IPv4 URL list | 5,979,648 | 2,160,640 | +1,024 | `cargo test -p ii --locked` (77 tests), isolated `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Web-share bidirectional file upload | 5,992,960 | 2,166,784 | +5,632 | `cargo test -p ii --locked` (78 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Web-share path access token | 6,001,664 | 2,169,344 | +2,560 | `cargo test -p ii --locked` (83 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Web upload path and `ii web` directory service | 6,038,016 | 2,180,608 | +11,264 | `cargo test -p ii --locked` (92 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Browser WebRTC LAN signalling and direct page | 6,068,736 | 2,189,824 | +9,216 | `cargo test -p ii --locked` (96 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| `ii web` Range, media MIME, and HEAD responses | 6,078,464 | 2,194,432 | +4,608 | `cargo test -p ii --locked` (98 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| `ii webrtc` text messages | 6,124,544 | 2,204,672 | +512 | Compared with the same `0.2.2` tunnel baseline (6,120,448 / 2,204,160); `cargo test -p ii --locked` (104 tests), `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Seven extension plan: multi-send, filters, rate, JSON, discover, bind, DAV | 6,315,008 | 2,265,600 | +60,928 | `cargo test -p ii --offline --no-fail-fast -- --test-threads=1` (121 tests), workspace check, `cargo build -p ii --release --locked`, `cargo bloat --crates`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Generic S3/R2 split and Azure Blob backend | 6,413,312 | 2,436,608 | +171,008 | `cargo test -p ii --locked` (131 tests), `cargo check -p ii-gui --locked`, `cargo bloat --crates`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Nine low-volume extensions: checksums, metadata, queue/watch, NAT probe, symlink policy, QUIC port, and `web --once` | 6,650,368 | 2,353,152 | -83,456 | `cargo test -p ii --locked` (146 tests), fmt check, diff check, `cargo bloat -p ii --release --crates -n 15`, standard `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Path diagnostics, relay selection, resumable web upload, and SOCKS5 | 6,727,168 | 2,377,728 | +24,576 | `cargo test -p ii --locked` (157 tests), fmt check, diff check, `cargo bloat -p ii --release --crates -n 15`, standard `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |
| Twelve lightweight network services: HTTP/paste/drop/PAC/speed, proxy/forwarding, and diagnostics | 6,866,432 | 2,576,384 | +198,656 | `cargo test -p ii --locked` (172 tests), fmt check, diff check, packed CLI help smoke test, `cargo bloat -p ii --release --crates -n 15`, standard `cargo build -p ii --release --locked`, UPX `5.1.0 --best --lzma`, `upx -t` |

Equivalent release rebuilds have varied by up to `3,072` UPX bytes; the latest
measurement is recorded with the pinned `5.1.0` packer.

## QR Dependency Audit

`D:\cache\n0-scan` contains `--depth 1` clones of all 120 public
`n0-computer` repositories. `squiggle` is an empty GitHub repository; all 119
non-empty repositories were scanned with `git grep` for QR-code dependencies and
source usage. The only direct generator found was `iroh-live`'s `qrcode 0.14.1`,
which depends on `image`. `ii` uses `qrcodegen 1.8.0` instead: it only supplies
the QR matrix, while `ii` emits its own inline SVG with no image or front-end
dependency chain.
