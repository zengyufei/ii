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
- 支持发送端校验和、单文件 metadata 保留、符号链接策略、FIFO 队列和目录监视。
- 可选通用 S3、Cloudflare R2、Azure Blob、WebDAV、FTP、SFTP 中转；传完可删除中转对象。
- 另有局域网网页、WebDAV、浏览器直传、TCP 隧道和自建 relay。

## 典型使用场景

### `ii web`

- 临时局域网文件站：当前目录一键公开浏览和下载；加 `--upload` 后也可上传。
- 手机相册导入：电脑开 `ii web --upload`，手机扫码上传照片或视频。
- 会议资料分发：用二维码让参会者直接下载。
- 本地静态资源站：给电视、平板和开发设备访问安装包、视频或网页文件。
- 临时文件收集箱：用 `ii drop` 建立只上传、不浏览、不下载的 `ii web` 子集服务，集中收集材料。
- 只读目录快捷入口：`ii http` 是默认只读 `ii web` 的子集入口，不增加任何传输能力。
- 离线局域网环境的文件门户：不依赖云盘、账号或外网。

### `ii paste`

- 在局域网分享 Wi-Fi 密码、会议码、日志片段、配置或短命令；手机扫码即可复制。
- 用 `raw` 地址让脚本读取临时文本，不必额外搭建文本服务。

### `ii dav`

- 把任意目录挂载为网络磁盘，可由 Windows Explorer、macOS Finder 或 Linux 文件管理器直接访问。
- 用手机文件管理器访问电脑目录，复制、移动、删除文件或创建目录。
- 把旧电脑或小主机作为轻量 NAS，共享一个目录。
- 让支持 WebDAV 的编辑器直接打开并保存远端文本或配置。
- 作为跨设备素材盘，让平板、手机和电脑同时访问同一个工作目录。
- 建立临时交付目录，客户或同事以网络盘方式取文件，不需要安装 `ii`。

### `ii socks5`

- 给浏览器、下载器和命令行工具提供普通 SOCKS5 出口。
- 让缺少内置代理选项的软件配合支持 SOCKS5 的转发器，经另一台机器访问网络。
- 让局域网设备借用一台具备网络、VPN 或特殊路由的电脑出口。
- 调试代理兼容性，验证 `CONNECT`、UDP、域名解析和 IPv6 行为。
- 给支持代理配置的游戏、媒体播放器或 IoT 设备提供代理地址。
- 临时比较企业网络、VPN 和家庭网络下的 DNS 与连接路径。

### `ii proxy`

- 给浏览器、下载器和其他支持 HTTP 代理的工具提供普通 HTTP 正向代理。
- 让局域网设备经一台具备 VPN、特殊路由或目标网络访问能力的电脑访问网络。

### `ii pac`

- 托管 PAC 地址，让浏览器自动对内网地址直连、其余地址走指定 HTTP 或 SOCKS5 代理。
- 在临时办公网、VPN 或测试网络中统一分发代理规则，不必逐台手动配置。

### `ii relay`

- 自建 Iroh relay，作为 `ii send --relay` 的指定跨网转发路径。
- 在内网、海外或特定地区部署 relay，避免完全依赖公共 relay。
- 在网络受限环境中验证 Iroh 的连接和转发能力。

### `ii tunnel`

- 暴露本机 TCP 服务，例如本地开发站、SSH、数据库或游戏服务器。
- 让没有公网 IP 的机器被远端访问。
- 用于临时远程协助，无需配置路由器端口转发。

### `ii tcp`

- 将本地监听端口转发到固定 TCP 目标，例如 SSH、数据库、开发站或内部服务。
- 在测试、短期联调和端口映射场景中转发一个已知 TCP 服务。

### `ii udp`

- 将 UDP 流量转发到固定目标，适合游戏、设备发现和自定义 UDP 协议调试。
- 让无法直接访问目标 UDP 服务的局域网设备经当前电脑中转。

### `ii discover`

- 扫描局域网内正在运行的 `ii` 服务。
- 找到同网段的发送任务、目录站和 WebDAV 服务，不必手输 IP。
- 在会议室、实验室或机房快速发现共享端点。

### `ii ping`

- 测量 TCP 建连延迟，快速判断某个服务地址是否可达、网络是否抖动。
- 对比企业网络、VPN、家庭网络或不同出口下的连接质量。

### `ii speed`

- 两台设备分别运行 `ii speed serve` 和 `ii speed`，测量真实局域网上下行吞吐。
- 排查 Wi-Fi 标称带宽很高但实际传文件很慢的链路问题，不依赖外网测速站。

### `ii wake`

- 向同网段休眠电脑或 NAS 发送 Wake-on-LAN 魔术包。
- 在远程传文件、备份或访问前先唤醒目标设备。

### `ii port`

- 一次检查同一主机的多个 TCP 端口是否开放、拒绝或超时。
- 排查防火墙、服务监听、端口映射和网络策略问题。

### `ii health`

- 检查 HTTP(S) 健康端点或裸 TCP 服务是否可用。
- 持续监控服务状态，只在状态变化时输出，适合轻量值守和脚本集成。

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

# 输出实际发送字节的校验和
ii send .\video.mp4 --checksum sha256

# 单文件保留 mtime、权限和只读属性
ii send .\video.mp4 --preserve-metadata
```

普通 `ii send` 在首次成功发送后退出。`-t` 最多同时服务 16 个接收端，另有最多 1000 个 FIFO 排队连接；并发接收共享发送端带宽，满队列时稍后重试。`--json` 时 stdout 只输出 JSON Lines。

`--checksum md5|sha256` 只在本地计算并输出实际发送字节，不写入 ticket，也不自动和接收端比较。`--preserve-metadata` 只适用于单个常规文件，会使用现有 tar 载荷，因此不支持 stdin、`--web`、断点续传和 MD5 秒传。

### 队列与目录监视

```powershell
ii queue .\a.zip .\b.zip
ii queue .\report.pdf --after 10s
ii queue .\folder --every 1h
ii watch .\incoming --interval 2s --stabilize 2s
```

`queue` 和 `watch` 只在当前进程内排队，不持久化；它们接受 `--rate`、后端、relay、`--local`、`--no-relay`、`--checksum`、`--preserve-metadata` 和 `--symlinks`，不接受 `-t`、`-c`、`-o`、`--web` 或 `--json`。`--quic-port <1..65535>` 可固定 P2P 的 Iroh UDP 端口。

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

断网或传到一半失败后，重新执行同一条 `ii recv` 就会继续接收；如果目标文件已经完整且内容相同，会直接跳过；如果同名但内容不同，会覆盖。`ii send` 和 `ii recv` 都会显示传输进度、速率和完成耗时；`ii recv --trace` 输出建连阶段、最终直连或 relay 路径和 RTT。`--stdout` 不能与 `--json` 同用。

`--checksum md5|sha256` 在接收完成后计算实际文件或目录 tar 流；不能与 `--stdout` 同用。`--quic-port` 只适用于 P2P ticket。

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

`ii send --web` 为单个文件或目录提供下载页。默认第一个完整 `/download` 响应写完后立即退出；加 `-t` 才持续服务到 `Ctrl+C`，并发下载时第一个完成会直接结束进程。访问下载页、上传和失败下载不会退出。`ii web` 显示 nginx 风格目录列表，省略目录时服务当前目录；`--upload` 才开放多文件上传，默认写入启动目录的 `./ii/`，`--path <dir>` 可改为指定目录。网页上传按 1 MiB 分块写入，断网或刷新后重新选择同一文件会自动继续。`ii dav` 直接读写所服务目录，不使用网页上传目录。

`ii dav --username <username> --password <password>` 启用 HTTP Basic Auth，两个参数必须同时提供。`--password` 会出现在 shell 历史和进程列表中，只适合你明确接受这一风险的场景。`--tls` 开启 HTTPS；未提供 `--cert` 与 `--key` 时生成仅当前进程有效的自签证书，客户端必须手动信任。公网请使用受信任 PEM 证书，或让 HTTPS 反向代理终止 TLS 并将 `ii dav` 绑定到 `127.0.0.1`。未启用 `--tls` 时，Basic Auth 凭据会以明文传输。

`--port 8080` 固定端口，`--bind ::` 只监听 IPv6；裸 `--token` 生成路径令牌，`--token <value>` 使用指定令牌。路径 token 不是账号鉴权。

`ii web --once` 只在第一次完整的普通文件 `GET 200` 后退出；HEAD、Range、目录页、404 和上传不会消耗这次机会，且不能和 `--upload` 同用。

### 轻量局域网服务

```powershell
# 只读目录站；支持浏览、Range、媒体播放和下载
ii http .\public

# 文字剪贴板；无参数时从 stdin 读取，raw 返回纯文本
ii paste "meeting code: 123456" --ttl 30m
Get-Content .\note.txt -Raw | ii paste --token

# 纯上传收集箱；默认写到启动目录的 .\ii\
ii drop .\incoming

# 为现有 HTTP/SOCKS5 代理托管 PAC 文件
ii pac --proxy socks5://192.168.1.10:1080

# 局域网吞吐测试服务端和客户端
ii speed serve --port 9000
ii speed http://192.168.1.10:9000/ --duration 15s
```

`ii http` 没有上传和写入接口；`ii drop` 只有多文件上传和断点续传，不列目录也不提供下载，指定目录直接作为上传目录。`ii paste` 根页可复制内容，`raw` 返回 `text/plain`，`--ttl` 到期自动关闭。`ii pac` 根 URL 返回 PAC 文件，内网、回环、`.local` 和无点主机名走 `DIRECT`，其他目标走指定代理。它们都支持 `--port`、`--bind`、`--token [value]`，终端输出二维码、LAN URL 和 `other:`；也会被 `ii discover` 发现。

### 局域网发现

```powershell
# 列出同一 LAN 中的 ii send -t、web/dav 和轻量 HTTP 服务
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

### SOCKS5 代理

```powershell
ii socks5
ii socks5 --port 1080 --username alice --password secret
```

`ii socks5` 是独立的普通网络代理，默认监听 `0.0.0.0` 的随机端口并打印实际地址。支持 SOCKS5 `CONNECT`、`UDP ASSOCIATE`、`BIND`、IPv4、IPv6 和域名目标；不经过 Iroh、ticket 或 relay。提供 `--username` 与 `--password` 时启用 SOCKS5 用户名密码认证，两个参数必须成对出现。

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

省略 `--port` 时随机监听。终端打印实际可用的 IPv4 URL；`0.0.0.0` 只用于监听，云服务器公网 IP 可能需要从控制台取得。`-k` 仅用于自签 HTTPS；指定 HTTP 或 HTTPS relay 后，传输强制经过该 relay。重复指定 `--relay` 时，`ii send` 探测各显式 relay 的可达性和延迟，选取最快可达节点；不影响默认 n0 relay。完整 TLS、NAT 与安全边界见 [ii.md](ii.md)。

## 图形界面

Release 同时提供 `ii-gui`。它支持默认自动路径、仅局域网、指定 HTTPS relay、S3 和 WebDAV；R2、Azure、FTP、SFTP 仅在 CLI 中提供。

## 诊断

```powershell
# 查看接收路径和耗时
ii recv ii1k7v...x9a --trace

# 检查本机网络、端口、权限和版本
ii doctor
ii doctor --nat
ii version
```

`ii doctor --nat` 会运行一次短生命周期 UDP/NAT/relay 探测，输出实际 UDP socket、IPv4/IPv6、NAT 映射、首选 relay 和 relay 在线结果；Iroh 未公开 hairpin 探测时会明确标记为不可用。

## 代理、转发与网络工具

```powershell
# HTTP 正向代理；可选 Basic 认证
ii proxy --port 8080
ii proxy --username alice --password secret

# 将本地 TCP/UDP 端口转发到固定目标
ii tcp db.internal:5432 --port 15432
ii udp game.internal:27015 --port 27015

# TCP connect 探测、端口检查、持续健康检查
ii ping api.example.com:443 --count 4
ii port api.example.com 80 443 8443
ii health https://api.example.com/health --interval 10s

# 发送 Wake-on-LAN 魔术包
ii wake aa:bb:cc:dd:ee:ff --broadcast 192.168.1.255
```

`ii proxy` 支持 HTTP/1.0、HTTP/1.1 absolute-form 请求与 `CONNECT`，普通 HTTP 请求一连接一请求，不做 TLS 解密、缓存或 WebSocket 代理。`ii tcp` 和 `ii udp` 是固定目标转发器；UDP 按客户端地址维护上游会话，空闲五分钟清理。`ii ping` 是 TCP connect 延迟探测，不发送 ICMP；`ii port` 并发检查 TCP 端口；`ii health` 对 HTTP(S) 仅将 `2xx/3xx` 视为健康，或检查裸 `host:port` 的 TCP 连通性。

## 详细手册

完整命令、配置和故障排查见 [ii.md](ii.md)。

## 变更记录

[CHANGELOG.md](CHANGELOG.md) · [CHANGELOG.en.md](CHANGELOG.en.md)

## 版本

版本以 GitHub Release 和 Git tag 为准。

## 许可证

MIT License，见 [LICENSE](LICENSE)。
