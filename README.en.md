<p align="center">
  <img src="logo.svg" alt="ii logo" width="96" height="96">
</p>

<h1 align="center">ii</h1>

<p align="center">
  A cross-platform file transfer CLI for quickly sending files, folders, and piped data.
</p>

<p align="center">
  <a href="https://github.com/zengyufei/ii/releases"><img alt="Release" src="https://img.shields.io/github/v/release/zengyufei/ii?style=for-the-badge&label=release"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-111111?style=for-the-badge"></a>
  <img alt="Platforms" src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-16784b?style=for-the-badge">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-CLI-be3f36?style=for-the-badge">
</p>

<p align="center">
  <a href="README.md">简体中文</a> · <strong>English</strong>
</p>

`ii` is built for temporary file transfer:

- The sender serves one successful receive by default, then exits
- P2P direct / LAN discovery with `ii discover` / public relay fallback / optional S3, WebDAV, FTP, and SFTP backends
- `ii send <file-or-folder> --web` download sharing / `ii web [directory]` directory browsing / `ii dav [directory]` WebDAV serving / `ii webrtc` browser-to-browser transfer / `ii tunnel` TCP port forwarding
- Receives resume automatically by default
- Existing files with the same MD5 are skipped
- Folders can be sent directly

## Quick Start

Run this on the sender:

```powershell
ii send .\video.mp4
```

`ii` prints a ticket:

```text
ii ticket:
ii1k7v...x9a

on the other computer:
ii recv ii1k7v...x9a
```

Run this on the receiver:

```powershell
ii recv ii1k7v...x9a
```

## Common Scenarios

Send a temporary file to a coworker:

```powershell
ii send .\report.pdf
ii recv ii1k7v...x9a
```

What the sender and receiver look like:

![Sender screenshot](screenshot/发送.png)

![Receiver screenshot](screenshot/接收.png)

Choose an output directory:

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

If the network drops halfway, run the same `ii recv` command again and it continues receiving. If the target file already exists with the same content, it is skipped. If the name matches but the content differs, it is overwritten.

`ii send` and `ii recv` both show live transfer progress and speed in the terminal, then print the final elapsed time when done. `--trace` switches to diagnostic output so you can see where the delay comes from.

## Send Folders

Folders can be sent directly:

```powershell
ii send .\my-folder
```

Receiver:

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

The result is `D:\Downloads\my-folder`, not a duplicated `my-folder\my-folder` nesting.

## Advanced Usage

### Send Control

```powershell
# Keep the same ticket available
ii send .\my-folder -t

# Copy the receive command or write it to a file
ii send .\video.mp4 -c
ii send .\video.mp4 -o recv.txt

# Pipe input and standard output
tar czf - .\project | ii send --name project.tar.gz
ii recv ii1k7v...x9a --stdout > project.tar.gz
```

`-t` serves up to 16 receivers concurrently and queues up to 1,000 more in first-in, first-out order. Concurrent receivers share sender bandwidth; retry later when the queue is full. Plain `ii send` still exits after its first successful transfer.

### LAN Web Services

```powershell
# Share a file or folder
ii send .\video.mp4 --web

# Browse a directory
ii web .\shared

# Transfer files and text between browsers
ii webrtc
```

All three print LAN URLs, other adapter URLs, and a terminal QR code. Without a directory, `ii web` serves the current directory. `--port 8080` fixes the port; `--bind ::` selects IPv6 only. Bare `--token` generates a path token, while `--token <value>` supplies one. `ii send --web` and `ii web` are read-only by default; pass `--upload` to enable multi-file uploads and `--path <dir>` to choose the upload directory. `ii discover` listens on the local network for three seconds and lists `ii send -t`, `ii web`, and `ii dav` services; discovery exposes tickets or URLs to the LAN and is not access control. These services have no account authentication and are for short-lived, trusted LAN use only. See [ii.md](ii.md) for the full rules and [webrtc.md](webrtc.md) for WebRTC limits.

### TCP Tunnel

```powershell
# A: make a service reachable from A available to ticket holders
ii tunnel -s 127.0.0.1:22

# B: use the ticket printed by A
ii tunnel -c ii1k7v...x9a
```

B listens on `127.0.0.1:8080` by default and increments the port when it is occupied. Use `--listen 0.0.0.0:8022` only to expose B's listener to its LAN. Traffic is end-to-end encrypted by Iroh; a ticket holder can use the target until A stops the service. See [ii.md](ii.md) for the full protocol and relay options.

### Storage Backends

| Backend | Send command | Notes |
| --- | --- | --- |
| S3 / R2 | `ii send .\video.mp4 --s3` | Prompts for configuration on first use |
| WebDAV | `ii send .\video.mp4 --webdav` | Supports portable tickets |
| FTP | `ii send .\video.mp4 --ftp` | Plaintext `ftp://` only |
| SFTP | `ii send .\video.mp4 --sftp` | Password and private-key authentication |

`--profile <name>` selects a backend configuration. `-p` writes WebDAV, FTP, or SFTP credentials into the ticket; tickets are not encrypted, so share them only with trusted receivers. `-d` attempts to delete the backend object after a successful receive. See [ii.md](ii.md), [ftp.md](ftp.md), and [sftp.md](sftp.md) for configuration and protocol limits.

## Desktop GUI

Releases also include `ii-gui`. It supports the default automatic path, local-only transfers, a chosen HTTPS relay, S3, and WebDAV. FTP and SFTP are CLI-only.

## Diagnostics

```powershell
# Inspect the receive path and timing
ii recv ii1k7v...x9a --trace

# Check local networking, ports, permissions, and version
ii doctor
ii version
```

## Self-hosted Relay

You only need this for a fixed relay endpoint.

```powershell
# HTTP relay
ii relay --port 8443
ii send .\video.mp4 --relay http://SERVER_PUBLIC_IP:8443

# Temporary self-signed HTTPS relay
ii relay --tls --port 8443
ii send .\video.mp4 --relay https://SERVER_PUBLIC_IP:8443 -k

# Existing PEM certificate
ii relay --tls --domain relay.example.com --port 8443 --cert D:\certs\fullchain.pem --key D:\certs\privkey.pem
ii send .\video.mp4 --relay https://relay.example.com:8443
```

Without `--port`, the relay chooses a free port. The terminal prints usable IPv4 URLs; `0.0.0.0` is bind-only, and a cloud public IP may need to come from the provider console. `-k` is only for self-signed HTTPS. An explicit HTTP or HTTPS relay forces traffic through that relay. See [ii.md](ii.md) for TLS, NAT, and security boundaries.

## Full Manual

Full command, configuration, and troubleshooting reference: [ii.md](ii.md).

## Changelog

[CHANGELOG.en.md](CHANGELOG.en.md) · [CHANGELOG.md](CHANGELOG.md)

## Version

Versions are defined by GitHub Releases and Git tags.

## License

MIT License. See [LICENSE](LICENSE).
