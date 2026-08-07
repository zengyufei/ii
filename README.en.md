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

`ii` is a CLI for temporary file transfer: it connects peer-to-peer by default, discovers services on the LAN, and automatically falls back to Iroh's default n0 relay when direct connectivity is unavailable.

- Send files, folders, multiple paths, or piped data. Receives resume, skip matching MD5 files, and overwrite conflicts.
- Compute local checksums, preserve single-file metadata, choose symlink behavior, and run FIFO queues or directory watches.
- Use generic S3, Cloudflare R2, Azure Blob, WebDAV, FTP, or SFTP as optional backends, with cleanup after receiving.
- It also provides LAN web sharing, WebDAV, browser transfer, TCP tunnels, and self-hosted relays.

## Common Use Cases

### `ii web`

- Run a temporary LAN file site from the current directory for browsing and downloads; add `--upload` to accept uploads.
- Import a phone photo library by starting `ii web --upload` on a computer and uploading photos or videos after scanning its QR code.
- Distribute meeting materials through a QR code for direct download.
- Serve local static assets such as installers, videos, or web files to TVs, tablets, and development devices.
- Use `ii drop` as the upload-only, no-browsing, no-download subset of `ii web` to collect files from several people.
- Use `ii http` as the read-only subset entry point for default `ii web`; it adds no transfer capability.
- Provide a file portal on an offline LAN without cloud storage, accounts, or Internet access.

### `ii paste`

- Share Wi-Fi passwords, meeting codes, log excerpts, configuration, or short commands on the LAN; phones can scan and copy them.
- Let scripts read temporary text through the `raw` URL without running another text service.

### `ii dav`

- Mount any directory as a network drive in Windows Explorer, macOS Finder, or a Linux file manager.
- Access a computer directory from a phone file manager to copy, move, delete, or create files and folders.
- Turn an old computer or small host into a lightweight NAS for one directory.
- Open and save remote text or configuration files from WebDAV-capable editors.
- Use one working directory as a shared asset drive for tablets, phones, and computers.
- Provide a temporary delivery directory that clients or colleagues can access as a network drive without installing `ii`.

### `ii socks5`

- Provide a standard SOCKS5 proxy endpoint for browsers, downloaders, and command-line tools.
- Route software without built-in proxy settings through another machine's network connection with a SOCKS5-capable proxyifier.
- Let LAN devices use a computer with Internet access, a VPN, or special routes as their proxy exit.
- Test proxy compatibility for `CONNECT`, UDP, DNS resolution, and IPv6.
- Provide a configurable proxy address to games, media players, or IoT devices that support one.
- Compare DNS and connection paths across enterprise, VPN, and home networks.

### `ii proxy`

- Provide an ordinary HTTP forward proxy for browsers, downloaders, and other tools that support HTTP proxies.
- Let LAN devices use a computer with a VPN, special routes, or access to the target network as their proxy exit.

### `ii pac`

- Host a PAC URL so browsers connect directly to LAN addresses and use a chosen HTTP or SOCKS5 proxy for other destinations.
- Distribute proxy rules across a temporary office network, VPN, or test network without configuring each machine manually.

### `ii relay`

- Self-host an Iroh relay as the explicit cross-network forwarding path for `ii send --relay`.
- Deploy relays on an intranet, overseas, or in a specific region instead of relying entirely on public relays.
- Validate Iroh connection and forwarding behavior on restricted networks.

### `ii tunnel`

- Expose a local TCP service such as a development site, SSH server, database, or game server.
- Make a machine without a public IP reachable remotely.
- Provide temporary remote assistance without configuring router port forwarding.

### `ii tcp`

- Forward a local listening port to a fixed TCP target such as SSH, a database, a development site, or an internal service.
- Forward a known TCP service for tests, short integration work, or temporary port mapping.

### `ii udp`

- Forward UDP traffic to a fixed target for games, device discovery, or custom UDP protocol debugging.
- Let LAN devices reach a UDP service through the current machine when they cannot reach it directly.

### `ii discover`

- Scan for running `ii` services on the LAN.
- Find same-subnet send jobs, directory sites, and WebDAV services without typing IP addresses.
- Discover shared endpoints quickly in meeting rooms, labs, and server rooms.

### `ii ping`

- Measure TCP connection latency to quickly determine whether a service is reachable or the network is unstable.
- Compare connection quality across enterprise, VPN, home, or different egress networks.

### `ii speed`

- Run `ii speed serve` on one device and `ii speed` on another to measure actual LAN download and upload throughput.
- Diagnose links whose advertised Wi-Fi bandwidth is high but whose file transfer speed is poor, without an Internet speed test.

### `ii wake`

- Send a Wake-on-LAN magic packet to a sleeping computer or NAS on the same subnet.
- Wake a target device before remote file transfer, backup, or access.

### `ii port`

- Check whether multiple TCP ports on one host are open, refused, or timed out in one command.
- Diagnose firewalls, service listeners, port mappings, and network policy issues.

### `ii health`

- Check an HTTP(S) health endpoint or a bare TCP service.
- Monitor a service continuously and print only state changes for lightweight supervision or scripts.

## Install

Linux x86_64 and Apple Silicon macOS:

```bash
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/zengyufei/ii/master/install.sh | bash
```

The default install path is `~/.local/bin/ii`; set `II_INSTALL_DIR` to choose another directory. For Windows x64, download the executable from [GitHub Releases](https://github.com/zengyufei/ii/releases). Releases also provide every published asset and its checksum.

## Quick Start

Sender:

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

Receiver:

```powershell
ii recv ii1k7v...x9a
```

What the sender and receiver look like:

![Sender screenshot](screenshot/发送.png)

![Receiver screenshot](screenshot/接收.png)

## Send

### Files, Folders, and Multiple Paths

```powershell
# File
ii send .\report.pdf

# Folder; receives as <output-directory>\my-folder
ii send .\my-folder

# Multiple files or folders; receives as <output-directory>\ii
ii send .\report.pdf .\images .\notes.txt

# Name the multiple-input collection root
ii send .\report.pdf .\images --name release
```

Multiple paths are packed into one tar stream, so older receivers still handle them as directory tars. Top-level input names must be unique.

### Pipes, Filters, and Rate Limits

```powershell
# Piped input needs an output file name
tar czf - .\project | ii send --name project.tar.gz

# Send matching files only; exclude overrides include
ii send .\project --include "**/*.rs" --exclude "target/**"

# All receivers share an 8 MiB/s total sender limit
ii send .\video.mp4 --rate 8MiB
```

`--include` and `--exclude` are repeatable and match `/`-separated paths relative to each input folder; they do not apply to `--web`. `--rate` accepts bytes/s, `KiB`, `MiB`, or `GiB`, and also limits web downloads and backend sends.

### Send Control

```powershell
# Keep the same ticket available
ii send .\my-folder -t

# Copy the receive command or write it to a file
ii send .\video.mp4 -c
ii send .\video.mp4 -o recv.txt

# JSON Lines for automation
ii send .\video.mp4 --json

# Print a checksum of the bytes being sent
ii send .\video.mp4 --checksum sha256

# Preserve mtime, permissions, and read-only metadata for one file
ii send .\video.mp4 --preserve-metadata
```

Plain `ii send` exits after its first successful transfer. `-t` serves up to 16 receivers concurrently and queues up to 1,000 more in first-in, first-out order. Concurrent receivers share sender bandwidth; retry later when the queue is full. With `--json`, stdout contains JSON Lines only.

`--checksum md5|sha256` computes and prints the local payload checksum. It is not stored in the ticket and is not compared automatically. `--preserve-metadata` accepts one regular file only and wraps it in the existing tar payload; it rejects stdin and `--web`, and disables resume and MD5 skip for that send.

### Queues and Directory Watches

```powershell
ii queue .\a.zip .\b.zip
ii queue .\report.pdf --after 10s
ii queue .\folder --every 1h
ii watch .\incoming --interval 2s --stabilize 2s
```

`queue` and `watch` are process-local and non-persistent. They accept `--rate`, storage backends, relay, `--local`, `--no-relay`, `--checksum`, `--preserve-metadata`, and `--symlinks`; they reject `-t`, `-c`, `-o`, `--web`, and `--json`. `--quic-port <1..65535>` fixes the Iroh UDP port for P2P jobs.

### Storage Backends

| Backend | Send command | Notes |
| --- | --- | --- |
| Generic S3 | `ii send .\video.mp4 --s3` | Configure a compatible endpoint, region, bucket, and path-style mode |
| Cloudflare R2 | `ii send .\video.mp4 --r2` | Separate R2 configuration with the fixed R2 endpoint |
| Azure Blob | `ii send .\video.mp4 --azure` | Shared Key or Container SAS |
| WebDAV | `ii send .\video.mp4 --webdav` | Supports portable tickets |
| FTP | `ii send .\video.mp4 --ftp` | Plaintext `ftp://` only |
| SFTP | `ii send .\video.mp4 --sftp` | Password and private-key authentication |

`--profile <name>` selects a backend configuration. S3, R2, and Azure tickets contain only signed object URLs, so receivers need no local profile. `-p` writes WebDAV, FTP, or SFTP credentials into the ticket; tickets are not encrypted, so share them only with trusted receivers. `-d` attempts to delete the backend object after a successful receive. Mounted SMB/NFS directories remain usable as local send paths; native SMB/NFS backends are not provided. See [ii.md](ii.md), [ftp.md](ftp.md), and [sftp.md](sftp.md) for configuration and protocol limits.

## Receive

```powershell
# Choose an output directory
ii recv ii1k7v...x9a -o D:\Downloads

# Write to standard output
ii recv ii1k7v...x9a --stdout > project.tar.gz

# JSON Lines for automation
ii recv ii1k7v...x9a --json
```

If the network drops halfway, run the same `ii recv` command again and it continues receiving. If the target file already exists with the same content, it is skipped. If the name matches but the content differs, it is overwritten. `ii send` and `ii recv` show progress, speed, and elapsed time; `ii recv --trace` prints connection stages, the final direct or relay path, and RTT. `--stdout` cannot be combined with `--json`.

`--checksum md5|sha256` computes the received file or directory tar stream after completion and cannot be combined with `--stdout`. `--quic-port` applies only to P2P tickets.

## Extended Capabilities

### LAN Web Sharing and WebDAV

```powershell
# Serve one file or folder as a download page
ii send .\video.mp4 --web

# Browse a directory; --upload enables standalone file uploads
ii web .\shared --upload --path .\uploads

# Mount a directory in a file manager; writable by default
ii dav .\shared
ii dav .\shared --read-only
ii dav .\shared --port 8443 --username alice --password secret --tls --domain dav.example.com --cert D:\certs\fullchain.pem --key D:\certs\privkey.pem
```

`ii send --web` serves a download page for one file or folder. It exits after the first complete `/download` response by default; add `-t` to keep serving until `Ctrl+C`, and the first concurrent download to finish ends the process. Visiting the page, uploads, and failed downloads do not exit. `ii web` displays an nginx-style directory listing and serves the current directory when no directory is given. `--upload` enables multi-file uploads only; their default destination is `./ii/` under the startup directory, while `--path <dir>` selects another directory. Uploads are written in 1 MiB chunks; after a disconnect or refresh, selecting the same file resumes it. `ii dav` reads and writes its served directory directly, not the web upload directory.

`ii dav --username <username> --password <password>` enables HTTP Basic Auth; both options are required together. `--password` is visible in shell history and process listings, so use it only where that is acceptable. `--tls` enables HTTPS. Without `--cert` and `--key`, ii generates a self-signed certificate valid only for the running process, which clients must explicitly trust. For public access, use a trusted PEM certificate or terminate HTTPS in a reverse proxy and bind `ii dav` to `127.0.0.1`. Without `--tls`, Basic Auth credentials travel in clear text.

`--port 8080` fixes the port, `--bind ::` listens on IPv6 only, and bare `--token` generates a path token while `--token <value>` uses the supplied token. A path token is not account authentication.

`ii web --once` exits only after the first complete ordinary-file `GET 200`. HEAD, Range, directory pages, 404 responses, and uploads do not consume the one-shot lifetime, and `--once` conflicts with `--upload`.

### Lightweight LAN Services

```powershell
# Read-only directory site with browsing, Range, media playback, and downloads
ii http .\public

# Text clipboard; without text, reads stdin; raw returns plain text
ii paste "meeting code: 123456" --ttl 30m
Get-Content .\note.txt -Raw | ii paste --token

# Upload-only drop box; defaults to .\ii\ under the startup directory
ii drop .\incoming

# Host a PAC file for an existing HTTP or SOCKS5 proxy
ii pac --proxy socks5://192.168.1.10:1080

# LAN throughput server and client
ii speed serve --port 9000
ii speed http://192.168.1.10:9000/ --duration 15s
```

`ii http` has no upload or write endpoint. `ii drop` provides resumable multi-file uploads only: it does not list or download files, and an explicit directory is the upload destination. `ii paste` provides a copyable root page and a `text/plain` `raw` endpoint; `--ttl` stops it at expiry. `ii pac` returns a PAC file at its root URL: private, loopback, `.local`, and plain host names use `DIRECT`; other destinations use the selected proxy. All five commands accept `--port`, `--bind`, and `--token [value]`, print a QR code, LAN URL, and `other:`, and advertise through `ii discover`.

### LAN Discovery

```powershell
# List ii send -t, web/dav, and lightweight HTTP services on the same LAN
ii discover

# JSON Lines output
ii discover --json
```

Discovery waits for three seconds and stays on the local network. It exposes tickets or token URLs to the LAN; it is not access control.

### Browser Transfer

```powershell
ii webrtc
```

`ii webrtc` transfers files and text between two browsers. It also supports `--port`, `--bind`, and `--token`. See [webrtc.md](webrtc.md) for limits.

### TCP Tunnel

```powershell
# A: make a service reachable from A available to ticket holders
ii tunnel -s 127.0.0.1:22

# B: use the ticket printed by A
ii tunnel -c ii1k7v...x9a
```

B listens on `127.0.0.1:8080` by default and increments the port when it is occupied. Use `--listen 0.0.0.0:8022` only to expose B's listener to its LAN. Traffic is end-to-end encrypted by Iroh; a ticket holder can use the target until A stops the service. See [ii.md](ii.md) for the full protocol and relay options.

### SOCKS5 Proxy

```powershell
ii socks5
ii socks5 --port 1080 --username alice --password secret
```

`ii socks5` is a standalone ordinary network proxy. It listens on a random `0.0.0.0` port by default and prints the actual address. It supports SOCKS5 `CONNECT`, `UDP ASSOCIATE`, `BIND`, IPv4, IPv6, and domain targets. It does not use Iroh, tickets, or relays. Supplying `--username` and `--password` enables SOCKS5 username/password authentication; both options are required together.

### Self-hosted Relay

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

Without `--port`, the relay chooses a free port. The terminal prints usable IPv4 URLs; `0.0.0.0` is bind-only, and a cloud public IP may need to come from the provider console. `-k` is only for self-signed HTTPS. An explicit HTTP or HTTPS relay forces traffic through that relay. Repeating `--relay` makes `ii send` probe the explicit relays and select the fastest reachable one; the default n0 relay behavior is unchanged. See [ii.md](ii.md) for TLS, NAT, and security boundaries.

## Desktop GUI

Releases also include `ii-gui`. It supports the default automatic path, local-only transfers, a chosen HTTPS relay, S3, and WebDAV. R2, Azure, FTP, and SFTP are CLI-only.

## Diagnostics

```powershell
# Inspect the receive path and timing
ii recv ii1k7v...x9a --trace

# Check local networking, ports, permissions, and version
ii doctor
ii doctor --nat
ii version
```

`ii doctor --nat` runs a short-lived UDP/NAT/relay probe and reports bound UDP sockets, IPv4/IPv6, NAT mapping behavior, the preferred relay, and relay reachability. Hairpin is explicitly unavailable because Iroh does not expose that probe.

## Proxy, Forwarding, and Network Tools

```powershell
# HTTP forward proxy, with optional Basic authentication
ii proxy --port 8080
ii proxy --username alice --password secret

# Forward a local TCP or UDP port to one fixed target
ii tcp db.internal:5432 --port 15432
ii udp game.internal:27015 --port 27015

# TCP connect latency, port checks, and a continuous health check
ii ping api.example.com:443 --count 4
ii port api.example.com 80 443 8443
ii health https://api.example.com/health --interval 10s

# Send a Wake-on-LAN magic packet
ii wake aa:bb:cc:dd:ee:ff --broadcast 192.168.1.255
```

`ii proxy` supports HTTP/1.0 and HTTP/1.1 absolute-form requests plus `CONNECT`. Ordinary HTTP requests use one connection per request; it does not decrypt TLS, cache, or proxy WebSockets. `ii tcp` and `ii udp` forward to one fixed target; UDP keeps one upstream session per client address and removes idle sessions after five minutes. `ii ping` measures TCP connect latency rather than sending ICMP. `ii port` checks TCP ports concurrently. `ii health` considers only HTTP(S) `2xx/3xx` healthy, or checks TCP connectivity for a bare `host:port`.

## Full Manual

Full command, configuration, and troubleshooting reference: [ii.md](ii.md).

## Changelog

[CHANGELOG.en.md](CHANGELOG.en.md) · [CHANGELOG.md](CHANGELOG.md)

## Version

Versions are defined by GitHub Releases and Git tags.

## License

MIT License. See [LICENSE](LICENSE).
