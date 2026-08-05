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

`ii` 面向临时传文件/夹：

直发目录 / 一次即走 / 持续发送 / 自动复制到粘贴板或落盘

P2P 直连 / 局域网发现 / `ii discover` / 公网 relay 回退 / 支持 `--s3` / `--webdav` / `--ftp` / `--sftp` 后端中转

`ii send <文件或文件夹> --web` 分享下载 / `ii web [目录]` 浏览目录 / `ii dav [目录]` 挂载目录 / `ii webrtc` 浏览器直传 / `ii tunnel` TCP 端口转发

断点续收 / 秒传跳过 / 冲突覆盖 / 支持传完清理中转

进度速率 / 完成耗时 / 支持诊断本机 / 支持自建中继

## 快速开始

在发送端执行：

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

在接收端执行：

```powershell
ii recv ii1k7v...x9a
```

## 常见场景

同事之间临时传一个文件：

```powershell
ii send .\report.pdf
ii recv ii1k7v...x9a
```

发送端和接收端的实际样子：

![发送端截图](screenshot/发送.png)

![接收端截图](screenshot/接收.png)

指定保存目录：

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

断网或传到一半失败后，重新执行同一条 `ii recv` 就会继续接收；如果目标文件已经完整且内容相同，会直接跳过；如果同名但内容不同，会覆盖。

`ii send` 和 `ii recv` 都会在终端里实时显示传输进度和速率；完成后会打印最终耗时。`--trace` 主要用于诊断，方便排查连接慢在哪里。

## 发送目录

目录可以直接发送：

```powershell
ii send .\my-folder
```

接收端：

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

接收结果是 `D:\Downloads\my-folder`，不会变成 `my-folder\my-folder` 两层。

## 进阶用法

### 发送控制

```powershell
# 持续提供同一 ticket
ii send .\my-folder -t

# 复制接收命令或写入文件
ii send .\video.mp4 -c
ii send .\video.mp4 -o recv.txt

# 管道输入和标准输出
tar czf - .\project | ii send --name project.tar.gz
ii recv ii1k7v...x9a --stdout > project.tar.gz
```

`-t` 最多同时服务 16 个接收端，另有最多 1000 个连接按先到先服务等待；并发接收会共享发送端带宽，满队列时稍后重试。普通 `ii send` 仍只成功发送一次后退出。

### 局域网网页

```powershell
# 分享一个文件或目录
ii send .\video.mp4 --web

# 浏览目录
ii web .\shared

# 浏览器之间直传文件和文本
ii webrtc
```

三者都会打印 LAN URL、其他网卡 URL 和终端二维码。`ii web` 省略目录时服务当前目录。`--port 8080` 固定端口，`--bind ::` 只监听 IPv6；裸 `--token` 生成路径令牌，也可用 `--token <value>` 指定。`ii send --web` 与 `ii web` 默认只读，传入 `--upload` 才开放多文件上传；`--path <dir>` 指定上传目录。`ii discover` 监听本地网络三秒，列出带 `-t` 的发送端、`ii web` 和 `ii dav`；发现公告会向同一 LAN 暴露 ticket 或 URL，不是访问控制。网页服务没有账号鉴权，只适合临时可信局域网。完整规则见 [ii.md](ii.md)，WebRTC 限制见 [webrtc.md](webrtc.md)。

### TCP 隧道

```powershell
# A：让持有 ticket 的设备访问 A 可连接的服务
ii tunnel -s 127.0.0.1:22

# B：使用 A 输出的 ticket
ii tunnel -c ii1k7v...x9a
```

B 默认监听 `127.0.0.1:8080`，端口被占用时自动递增；`--listen 0.0.0.0:8022` 才会暴露给 B 的局域网。流量经 Iroh 端到端加密，ticket 持有者可访问目标直到 A 停止服务。完整用法见 [ii.md](ii.md)。

### 后端中转

| 后端 | 发送命令 | 说明 |
| --- | --- | --- |
| S3 / R2 | `ii send .\video.mp4 --s3` | 首次使用按提示配置 |
| WebDAV | `ii send .\video.mp4 --webdav` | 支持便携 ticket |
| FTP | `ii send .\video.mp4 --ftp` | 仅明文 `ftp://` |
| SFTP | `ii send .\video.mp4 --sftp` | 支持密码和私钥 |

`--profile <name>` 选择后端配置；`-p` 将 WebDAV、FTP 或 SFTP 凭据写入 ticket，ticket 未加密，只能交给可信接收方；`-d` 在接收成功后尝试删除中转对象。配置和协议限制见 [ii.md](ii.md)、[ftp.md](ftp.md)、[sftp.md](sftp.md)。

## 图形界面

Release 同时提供 `ii-gui`。它支持默认自动路径、仅局域网、指定 HTTPS relay、S3 和 WebDAV；FTP/SFTP 仅在 CLI 中提供。

## 诊断

```powershell
# 查看接收路径和耗时
ii recv ii1k7v...x9a --trace

# 检查本机网络、端口、权限和版本
ii doctor
ii version
```

## 自托管 Relay

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

## 详细手册

完整命令、配置和故障排查见 [ii.md](ii.md)。

## 变更记录

[CHANGELOG.md](CHANGELOG.md) · [CHANGELOG.en.md](CHANGELOG.en.md)

## 版本

版本以 GitHub Release 和 Git tag 为准。

## 许可证

MIT License，见 [LICENSE](LICENSE)。
