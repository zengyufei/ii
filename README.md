<p align="center">
  <img src="logo.svg" alt="ii logo" width="96" height="96">
</p>

<h1 align="center">ii</h1>

<p align="center">
  一个跨平台文件传输 CLI，用来快速发送文件、目录和管道数据。
</p>

<p align="center">
  <a href="https://github.com/zengyufei/ii/releases"><img alt="Release" src="https://img.shields.io/github/v/release/zengyufei/ii?style=for-the-badge&label=release"></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-111111?style=for-the-badge"></a>
  <img alt="Platforms" src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-16784b?style=for-the-badge">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-CLI-be3f36?style=for-the-badge">
</p>

<p align="center">
  <strong>简体中文</strong> · <a href="README.en.md">English</a>
</p>

`ii` 是临时文件传输命令行工具：默认 P2P 直连，局域网可发现；无法直连时自动降级到 Iroh 默认的 n0 relay。

- 发送文件、目录、多文件和管道数据；接收支持断点续传、同 MD5 跳过和冲突覆盖。
- 可选通用 S3、Cloudflare R2、Azure Blob、WebDAV、FTP、SFTP 中转；传完可删除中转对象。
- 另有局域网网页、WebDAV、浏览器直传、TCP 隧道和自建 relay。

## 安装

Linux x86_64 和 Apple Silicon macOS：

```bash
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/zengyufei/ii/master/install.sh | bash
```

默认安装到 `~/.local/bin/ii`；设置 `II_INSTALL_DIR` 可指定安装目录。Windows x64 请从 [GitHub Releases](https://github.com/zengyufei/ii/releases) 下载。所有发布资产和校验和也可在 Releases 获取。

## 快速开始

发送端：

```powershell
ii send .\video.mp4
```

`ii` 会输出一段 ticket：

```text
ii ticket:
ii1k7v...x9a

on the other computer:
ii recv ii1k7v...x9a
```

接收端：

```powershell
ii recv ii1k7v...x9a
```

发送端和接收端的实际样子：

![发送端截图](screenshot/发送.png)

![接收端截图](screenshot/接收.png)

## 发送

### 文件、目录和多文件

```powershell
# 文件
ii send .\report.pdf

# 目录；接收为 <输出目录>\my-folder
ii send .\my-folder

# 多个文件或目录；接收为 <输出目录>\ii
ii send .\report.pdf .\images .\notes.txt

# 指定多文件集合的根目录名
ii send .\report.pdf .\images --name release
```

多文件会打包为一个 tar 流，旧版接收端仍按目录 tar 接收。多个输入项的顶层名称不能重复。

### 管道、筛选和限速

```powershell
# 管道输入需要指定接收文件名
tar czf - .\project | ii send --name project.tar.gz

# 只发送匹配文件；exclude 优先于 include
ii send .\project --include "**/*.rs" --exclude "target/**"

# 所有接收端共享 8 MiB/s 总发送带宽
ii send .\video.mp4 --rate 8MiB
```

`--include` 和 `--exclude` 可重复使用，匹配输入目录内以 `/` 分隔的相对路径；它们不适用于 `--web`。`--rate` 接受 bytes/s、`KiB`、`MiB`、`GiB`，也限制网页下载和各中转后端的发送流量。

### 发送控制

```powershell
# 持续提供同一 ticket
ii send .\my-folder -t

# 复制接收命令或写入文件
ii send .\video.mp4 -c
ii send .\video.mp4 -o recv.txt

# 面向自动化的 JSON Lines
ii send .\video.mp4 --json
```

普通 `ii send` 在首次成功发送后退出。`-t` 最多同时服务 16 个接收端，另有最多 1000 个 FIFO 排队连接；并发接收共享发送端带宽，满队列时稍后重试。`--json` 时 stdout 只输出 JSON Lines。

### 后端中转

| 后端 | 发送命令 | 说明 |
| --- | --- | --- |
| 通用 S3 | `ii send .\video.mp4 --s3` | 配置兼容 endpoint、region、bucket 和 path-style |
| Cloudflare R2 | `ii send .\video.mp4 --r2` | 独立 R2 配置，固定 R2 endpoint |
| Azure Blob | `ii send .\video.mp4 --azure` | Shared Key 或 Container SAS |
| WebDAV | `ii send .\video.mp4 --webdav` | 支持便携 ticket |
| FTP | `ii send .\video.mp4 --ftp` | 仅明文 `ftp://` |
| SFTP | `ii send .\video.mp4 --sftp` | 支持密码和私钥 |

`--profile <name>` 选择后端配置；S3、R2 和 Azure ticket 只包含对象 URL，接收端无需本机配置。`-p` 将 WebDAV、FTP 或 SFTP 凭据写入 ticket，ticket 未加密，只能交给可信接收方；`-d` 在接收成功后尝试删除中转对象。SMB/NFS 已挂载目录可直接作为本地发送路径，不提供原生中转。配置和协议限制见 [ii.md](ii.md)、[ftp.md](ftp.md)、[sftp.md](sftp.md)。

## 接收

```powershell
# 指定保存目录
ii recv ii1k7v...x9a -o D:\Downloads

# 输出到标准输出
ii recv ii1k7v...x9a --stdout > project.tar.gz

# 面向自动化的 JSON Lines
ii recv ii1k7v...x9a --json
```

断网或传到一半失败后，重新执行同一条 `ii recv` 就会继续接收；如果目标文件已经完整且内容相同，会直接跳过；如果同名但内容不同，会覆盖。`ii send` 和 `ii recv` 都会显示传输进度、速率和完成耗时；`--trace` 输出连接诊断。`--stdout` 不能与 `--json` 同用。

## 扩展能力

### 局域网网页与 WebDAV

```powershell
# 为一个文件或目录提供下载页
ii send .\video.mp4 --web

# 浏览目录；加 --upload 后允许上传独立文件
ii web .\shared --upload --path .\uploads

# 用系统文件管理器挂载目录；默认可读写
ii dav .\shared
ii dav .\shared --read-only
ii dav .\shared --port 8443 --username alice --password secret --tls --domain dav.example.com --cert D:\certs\fullchain.pem --key D:\certs\privkey.pem
```

`ii send --web` 为单个文件或目录提供下载页。`ii web` 显示 nginx 风格目录列表，省略目录时服务当前目录；`--upload` 才开放多文件上传，默认写入启动目录的 `./ii/`，`--path <dir>` 可改为指定目录。`ii dav` 直接读写所服务目录，不使用网页上传目录。

`ii dav --username <username> --password <password>` 启用 HTTP Basic Auth，两个参数必须同时提供。`--password` 会出现在 shell 历史和进程列表中，只适合你明确接受这一风险的场景。`--tls` 开启 HTTPS；未提供 `--cert` 与 `--key` 时生成仅当前进程有效的自签证书，客户端必须手动信任。公网请使用受信任 PEM 证书，或让 HTTPS 反向代理终止 TLS 并将 `ii dav` 绑定到 `127.0.0.1`。未启用 `--tls` 时，Basic Auth 凭据会以明文传输。

`--port 8080` 固定端口，`--bind ::` 只监听 IPv6；裸 `--token` 生成路径令牌，`--token <value>` 使用指定令牌。路径 token 不是账号鉴权。

### 局域网发现

```powershell
# 列出同一 LAN 中的 ii send -t、ii web 和 ii dav
ii discover

# 输出 JSON Lines
ii discover --json
```

发现等待三秒，只在本地网络内工作；公告会暴露 ticket 或 token URL，不是访问控制。

### 浏览器直传

```powershell
ii webrtc
```

`ii webrtc` 打开网页后可在两台浏览器之间传文件和文本。它同样支持 `--port`、`--bind` 和 `--token`。限制见 [webrtc.md](webrtc.md)。

### TCP 隧道

```powershell
# A：让持有 ticket 的设备访问 A 可连接的服务
ii tunnel -s 127.0.0.1:22

# B：使用 A 输出的 ticket
ii tunnel -c ii1k7v...x9a
```

B 默认监听 `127.0.0.1:8080`，端口被占用时自动递增；`--listen 0.0.0.0:8022` 才会暴露给 B 的局域网。流量经 Iroh 端到端加密，ticket 持有者可访问目标直到 A 停止服务。完整用法见 [ii.md](ii.md)。

### 自建 Relay

普通传文件不需要自建 relay。需要固定中继入口时：

```powershell
# HTTP relay
ii relay --port 8443
ii send .\video.mp4 --relay http://服务器公网IP:8443

# 临时自签 HTTPS relay
ii relay --tls --port 8443
ii send .\video.mp4 --relay https://服务器公网IP:8443 -k

# 使用已有 PEM 证书
ii relay --tls --domain relay.example.com --port 8443 --cert D:\certs\fullchain.pem --key D:\certs\privkey.pem
ii send .\video.mp4 --relay https://relay.example.com:8443
```

省略 `--port` 时随机监听。终端打印实际可用的 IPv4 URL；`0.0.0.0` 只用于监听，云服务器公网 IP 可能需要从控制台取得。`-k` 仅用于自签 HTTPS；指定 HTTP 或 HTTPS relay 后，传输强制经过该 relay。完整 TLS、NAT 与安全边界见 [ii.md](ii.md)。

## 图形界面

Release 同时提供 `ii-gui`。它支持默认自动路径、仅局域网、指定 HTTPS relay、S3 和 WebDAV；R2、Azure、FTP、SFTP 仅在 CLI 中提供。

## 诊断

```powershell
# 查看接收路径和耗时
ii recv ii1k7v...x9a --trace

# 检查本机网络、端口、权限和版本
ii doctor
ii version
```

## 详细手册

完整命令、配置和故障排查见 [ii.md](ii.md)。

## 变更记录

[CHANGELOG.md](CHANGELOG.md) · [CHANGELOG.en.md](CHANGELOG.en.md)

## 版本

版本以 GitHub Release 和 Git tag 为准。

## 许可证

MIT License，见 [LICENSE](LICENSE)。
