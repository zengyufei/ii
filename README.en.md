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
- It briefly probes for a usable path through complex networks, with S3, WebDAV, FTP, and SFTP backend options
- `ii send <file-or-folder> --web` opens a temporary LAN download page, while `ii web [directory]` browses a LAN directory
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

The sender serves one receiver by default. Use `-t` to keep it running:

```powershell
ii send .\my-folder -t
```

Copy the receive command to the clipboard:

```powershell
ii send .\video.mp4 -c
```

Write the receive command to a file:

```powershell
ii send .\video.mp4 -o recv.txt
```

Send from stdin:

```powershell
tar czf - .\project | ii send --name project.tar.gz
```

Receive to stdout:

```powershell
ii recv ii1k7v...x9a --stdout > project.tar.gz
```

Open a temporary LAN download page:

```powershell
ii send .\video.mp4 --web
ii send .\my-folder --web
ii send .\video.mp4 --web --token A1b2C3d4E5f6G7h8
ii send .\video.mp4 --web --path .\uploads
ii web
ii web .\shared --token A1b2C3d4E5f6G7h8 --path .\uploads
```

The command displays a QR code directly above the primary LAN URL for opening the download page, then lists the remaining physical and virtual adapter IPv4 URLs under `other:`. The QR code at the top of that page points directly to `/download` for phone downloads. The page can also upload multiple files into `./ii/` under the directory where the command started. Use `--path <dir>` to write directly into a different directory; relative paths are based on the startup directory, and the directory is created on the first upload. Directory uploads are not supported. Folders download as `.tar` archives. Press `Ctrl+C` to stop the server. Optional `--token <value>` adds a path access token to the page, download, and upload URLs; it must be 16 to 128 ASCII letters, digits, `-`, or `_`, and omitting it keeps the unprotected URLs. This mode has no account authentication and is intended only for short-lived, trusted LAN sharing.

Without a path, `ii web` serves the directory where the command starts; with a path, it accepts only an existing directory. Its page provides recursive nginx-style directory browsing, normal file responses, and multi-file uploads. The terminal still shows the QR code, primary IPv4 LAN URL, and `other:` adapter URLs, but the directory page has no QR code. `--token` and `--path` follow the same rules as `ii send ... --web`; `-p` does not apply to `ii web`.

Prefer local network paths and avoid public relays:

```powershell
ii send .\file.zip --local
ii recv ii1k7v...x9a --local
```

Use WebDAV as a transfer backend:

```powershell
ii send .\video.mp4 --webdav
ii recv ii1k7v...x9a
```

Use FTP or SFTP as a transfer backend:

```powershell
ii send .\video.mp4 --ftp
ii send .\video.mp4 --sftp
ii recv ii1k7v...x9a
```

FTP only supports plaintext `ftp://`; its credentials, file data, and control commands can be read on the network. SFTP supports password and SSH private-key authentication. On every connection, `ii` prints and accepts the server SSH SHA-256 host-key fingerprint without storing it; a server can therefore still be replaced by a man-in-the-middle.

Select a backend profile:

```powershell
ii send .\video.mp4 --s3 --profile work
ii send .\video.mp4 --webdav --profile nas
ii send .\video.mp4 --ftp --profile legacy
ii send .\video.mp4 --sftp --profile server
```

If the receiver has no backend config, create a portable ticket:

```powershell
ii send .\video.mp4 --webdav -p
ii send .\video.mp4 --ftp -p
ii send .\video.mp4 --sftp -p
```

`-p` writes the WebDAV/FTP URL, username, and password into the ticket. An SFTP password ticket includes the password; an SFTP private-key ticket includes the key text and its passphrase. Tickets are encoded, not encrypted, so use this only when you trust the ticket recipient.

After a successful receive, configuration from a `-p` ticket is written to the receiver's local `ii.toml`; a portable SFTP private key is saved as a separate key file and the config stores only its path. To remove the backend object after receive, add `-d`:

```powershell
ii send .\video.mp4 --webdav -p -d
ii send .\video.mp4 --sftp -p -d
```

An SFTP private-key profile looks like this:

```toml
[storage.sftp.server]
host = "sftp.example.com"
port = 22
username = "ii"
remote_dir = "ii/"
auth = "private-key"
private_key_path = "/home/you/.ssh/id_ed25519"
private_key_passphrase = "optional-passphrase"
```

## Desktop GUI

Releases also include `ii-gui`. It reuses `ii` transfer logic and supports the default automatic path, local-only transfers, a chosen HTTPS relay, S3, and WebDAV; it stores a local transfer queue and these profiles. FTP and SFTP are currently CLI-only.

## Diagnostics

Trace why a receive is slow:

```powershell
ii recv ii1k7v...x9a --trace
ii recv ii1k7v...x9a --local --trace
```

Check local networking, ports, permissions, and version information:

```powershell
ii doctor
ii version
```

## Self-hosted Relay

You do not need to understand relay hosting to send ordinary files. This section is only for running your own relay service or using a fixed relay entrypoint in a company network.

Start a self-signed HTTPS relay:

```powershell
ii relay --public https://SERVER_PUBLIC_IP:8443
```

You can use a domain too:

```powershell
ii relay --public https://relay.example.com
```

`--public` is the public HTTPS address used by clients and must be `https://host[:port]`. On first start, `ii` generates and persists a self-signed certificate and key in the relay state directory. It listens on the public URL port, or on `443` when the URL has no port. Use `-H` for a different local backend port behind NAT or a reverse proxy:

```powershell
ii relay --public https://relay.example.com:8443 -H 9443
```

Send through the relay:

```powershell
ii send .\video.mp4 --relay https://SERVER_PUBLIC_IP:8443 -k
```

`-k` accepts the self-signed certificate and puts that policy in the ticket; the receiver needs no certificate installation or relay configuration. A first connection can still be replaced by a man-in-the-middle.

With a domain and PEM certificate files, use manual TLS:

```powershell
ii relay --tls relay.example.com -H 8443 --cert D:\certs\fullchain.pem --key D:\certs\privkey.pem
ii send .\video.mp4 --relay https://relay.example.com:8443
```

Manual TLS does not use `-k`; clients use normal system TLS verification. Both `--relay` modes force HTTPS relay-only transport and skip UDP and direct paths. See [ii.md](ii.md) for ports, state paths, and the security boundary. FTP and SFTP configuration, tickets, and security limits are documented in [ftp.md](ftp.md) and [sftp.md](sftp.md).

## Full Manual

The full command reference, port roles, TLS sources, config paths, diagnostics, and implementation mapping are documented in [ii.md](ii.md).

## Changelog

Release changes are documented in [CHANGELOG.en.md](CHANGELOG.en.md). The default Chinese version is [CHANGELOG.md](CHANGELOG.md).

## Version

The current version is managed by Git tags. This repository currently uses `v0.1.15`.

## License

This project uses the MIT License. You can use, modify, and distribute it freely. See [LICENSE](LICENSE).
