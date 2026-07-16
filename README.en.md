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
- It briefly probes for a usable path through complex networks
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

`ii recv` shows live transfer progress and speed in the terminal; if you enable `--trace`, it switches to diagnostic output so you can see where the delay comes from.

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

Prefer local network paths and avoid public relays:

```powershell
ii send .\file.zip --local
ii recv ii1k7v...x9a --local
```

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

Start a relay:

```powershell
ii relay
```

Default ports:

- HTTP: `80`
- HTTPS: `443`
- QUIC: `7842`
- metrics: `9090`, disabled by default

If `80/443` are already used by Nginx, run `ii relay` on non-standard backend ports and forward through the front proxy. `7842/udp` is an independent QUIC port and cannot be replaced by a normal HTTP reverse proxy.

Production TLS primarily uses automatic ACME issuance. Development mode can use `--dev` for plain HTTP.

## Full Manual

The full command reference, port roles, TLS sources, config paths, diagnostics, and implementation mapping are documented in [ii.md](ii.md).

## Changelog

Release changes are documented in [CHANGELOG.en.md](CHANGELOG.en.md). The default Chinese version is [CHANGELOG.md](CHANGELOG.md).

## Version

The current version is managed by Git tags. This repository currently uses `v0.1.4`.

## License

This project uses the MIT License. You can use, modify, and distribute it freely. See [LICENSE](LICENSE).
