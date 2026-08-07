# 变更记录

本文件记录 `ii` 的重要变更。默认中文版本在这里，英文版本见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## 0.3.7 - 2026-08-07

### 新增

- `ii send` 和 `ii recv` 支持 `--checksum md5|sha256`；校验和仅在本地计算和输出，不写入 ticket，也不自动比较。
- `ii send <file> --preserve-metadata` 可用现有 tar 载荷保留单文件的 mtime、权限和只读属性；目录和多路径归档支持 `--symlinks follow|preserve|reject`。
- 新增进程内 FIFO `ii queue` 与轮询式 `ii watch`，支持延迟、定时、稳定检测和现有发送后端。
- `ii send`、`ii recv`、`ii watch`、`ii queue` 支持 `--quic-port` 固定 P2P Iroh UDP 端口；`ii doctor --nat` 输出短生命周期的 UDP、NAT 和 relay 探测结果。
- `ii web --once` 在第一次完整普通文件下载后关闭；目录页、HEAD、Range、上传和失败请求不会触发关闭。
- `ii recv --trace` 输出 Iroh 实际选中的 LAN、直连或 relay 路径及 RTT。
- `ii send`、`ii watch`、`ii queue` 的 `--relay` 可重复指定；多个显式 relay 会按探测到的可达性和延迟择优，默认 n0 relay 行为不变。
- `ii send --web` 与 `ii web --upload` 支持浏览器分块上传续传，断网或刷新后重新选择同一文件即可继续。
- 新增 `ii socks5` 普通 SOCKS5 代理，支持 `CONNECT`、`UDP ASSOCIATE`、`BIND`、IPv4、IPv6、域名目标和可选用户名密码认证。
- 新增 `ii http` 只读目录站、`ii paste` 文本分享、`ii drop` 续传上传收集箱、`ii pac` PAC 托管与 `ii speed` 双向 HTTP chunked 测速；它们复用 LAN URL、二维码、token 和发现服务。
- 新增 `ii proxy` HTTP 正向代理、`ii tcp`/`ii udp` 固定目标转发、`ii ping` TCP connect 延迟探测、`ii port` 并发端口检查、`ii health` HTTP(S)/TCP 健康检查和 `ii wake` Wake-on-LAN。

## 0.3.4 - 2026-08-06

### 变更

- `ii send --web` 默认在第一个完整下载完成后退出；加 `-t` 后才持续提供下载。访问页面、上传或失败下载不会结束服务。

## 0.3.3 - 2026-08-05

### 新增

- `ii dav` 支持 HTTP Basic Auth：`--username` 与 `--password` 必须成对提供。
- `ii dav` 支持 HTTPS：`--tls` 可生成临时自签证书，`--domain`、`--cert`、`--key` 支持自定义域名和 PEM 证书。

## 0.3.1 - 2026-08-05

### 新增

- `ii send --r2` 使用独立的 Cloudflare R2 profile；`ii send --azure` 支持 Azure Block Blob 的 Shared Key 或 Container SAS。

### 变更

- `ii send --s3` 现在只表示通用 S3 兼容对象存储，首次配置 endpoint、region、bucket、凭据和 path-style。
- 旧 `[storage.s3.<name>]` 的 `provider = "cloudflare-r2"` 不再迁移；改写为 `[storage.r2.<name>]` 并使用 `--r2`。已生成的签名对象 URL ticket 仍可接收。

## 0.3.0 - 2026-08-05

### 新增

- `ii send` 支持多文件/目录混合发送、`--include`/`--exclude` 筛选、`--rate` 总带宽限制和 `--json` JSON Lines 事件。
- 新增 `ii discover` 局域网发现，以及 `ii web`、`ii dav`、`ii send --web` 的 `--bind` IPv4/IPv6 监听。
- 新增读写 `ii dav` 局域网 WebDAV 服务，支持桌面客户端常用请求、Range、chunked PUT 和进程内锁。

## 0.2.9 - 2026-08-04

### 变更

- `ii send -t` 改为最多 16 个并发接收任务和最多 1000 个 FIFO 排队连接；单个接收端断线或超时不再阻塞其他空闲槽位的断点续传。

## 0.2.8 - 2026-08-03

### 变更

- `ii relay` 改为无参启动 HTTP relay，默认随机端口，支持 `--port` 固定端口；终端输出可访问 IPv4 URL 和 `other:` 网卡 URL。
- `--tls` 改为可选自签 HTTPS 开关，`--domain` 指定 TLS 域名，`--cert` 和 `--key` 可替换自动证书；删除 `--public` 与 `-H`。
- `ii send --relay` 与 `ii tunnel -s --relay` 支持 HTTP 或 HTTPS URL；`-k` 仅可用于 HTTPS relay。

## 0.2.7 - 2026-08-03

### 变更

- `ii send ... --web` 与 `ii web` 默认关闭网页上传；只有 `--upload` 才显示上传控件并开放上传接口，未配 `--upload` 的 `--path` 会被忽略。
- `ii send ... --web`、`ii web` 与 `ii webrtc` 的裸 `--token` 自动生成并打印 32 字符路径访问令牌；`--token <value>` 与 `--token=<value>` 保持可用。

## 0.2.6 - 2026-08-03

### 新增

- `ii send ... --web`、`ii web` 与 `ii webrtc` 支持 `--port <port>` 指定 HTTP 监听端口；未提供时仍随机选择。
- 新增 `ii help [command]`，可显示命令总览或指定命令的帮助。

## 0.2.5 - 2026-08-01

### 修复

- 修复 Unix 平台构建测试时缺少 `PathBuf` 导入的问题。

## 0.2.4 - 2026-08-01

### 变更

- 将 CLI 内核按命令、服务、传输、后端、Web、ticket、存储和 relay 职责拆分为独立模块；命令行、GUI 门面、ticket 编码和网络协议保持兼容。

## 0.2.3 - 2026-08-01

### 新增

- `ii webrtc` 支持向当前选中设备发送文本消息；长文本按 UTF-8 字节以不超过 1 MiB 的分片传输，接收端重组为一条消息并提供复制按钮，不保存聊天记录。

## 0.2.2 - 2026-08-01

### 新增

- 新增 `ii tunnel -s <target-host:port>` 与 `ii tunnel -c <ticket>`，通过现有 Iroh 直连或 relay 临时转发 TCP；ticket 内含访问密钥和指定 relay 的 TLS 信任策略。

## 0.2.1 - 2026-07-31

### 新增

- `ii web` 的普通文件支持 HTTP 单区间 Range、`HEAD`、常见媒体/PDF/图片 MIME；浏览器可原生播放并断点续传下载。

## 0.2.0 - 2026-07-31

### 新增

- 新增 `ii webrtc [--token <value>]`：临时提供局域网浏览器间 WebRTC 文件直传房间，终端输出二维码和全部 IPv4 LAN URL；文件不经过 `ii` 进程，不使用公网 STUN/TURN。

### 修复

- 修复 `ii webrtc` 在移动浏览器之间因 mDNS host candidate 无法解析或 ICE gathering 不结束而无法建立 DataChannel 的问题；信令使用访问服务的客户端局域网 IPv4 并即时转发候选。
- `ii webrtc` 页面在加入房间前实际验证 ICE host candidate；浏览器禁用或阻止 WebRTC 时明确提示，避免只能发现设备但无法传输。

### 文档

- 新增 [webrtc.md](webrtc.md)，说明局域网范围、浏览器要求、内存限制、路径令牌和不支持的能力。

## 0.1.19 - 2026-07-31

### 新增

- 新增 `ii web [目录]`：临时提供局域网递归目录浏览、普通文件访问与多文件上传；支持 `--token` 和 `--path`，终端输出二维码与全部 IPv4 LAN URL。
- `ii send ... --web --path <目录>` 可指定网页上传文件直接写入的目录；相对路径按启动目录解析，未指定时仍写入 `./ii/`。

## 0.1.18 - 2026-07-30

### 新增

- `--web` 分享页支持多文件上传，文件流式写入启动目录下的 `./ii/`；同名文件覆盖。
- 新增 `ii send ... --web --token <value>` 路径访问令牌；网页、下载、上传、终端 URL 与二维码均使用该路径，缺失或错误路径返回 `404`。

### 文档

- 更新中英文 README 和命令手册，说明网页上传和 `--token` 的用法及限制。

## 0.1.17 - 2026-07-30

### 新增

- 新增 `ii send <文件或文件夹> --web`，临时提供局域网 HTTP 分享页；终端和网页均提供二维码，目录下载为 `.tar`。
- 输出主局域网 URL 和其余物理、虚拟网卡 IPv4 URL，并为手机页面调整布局。

## 0.1.16 - 2026-07-30

### 修复

- 修正 Release 流水线的 UPX 平台处理：Windows 解压前清理残留目录，macOS 压缩 Mach-O 时传入 `--force-macos`。

## 0.1.15 - 2026-07-30

### 变更

- Release 流水线继续报告三端 CLI 的 UPX 压缩大小，但不再以大小限制阻断发布。

## 0.1.14 - 2026-07-30

### 变更

- Release 流水线仅构建和发布 CLI 三端产物；修正 Linux 与 macOS 使用 UPX 路径时误写入 UPX 保留环境变量的问题。

## 0.1.13 - 2026-07-30

### 新增

- 新增 `ii send --ftp` 和 `ii send --sftp`，支持 FTP 与 SFTP 中转发送文件、stdin 和目录。
- 新增 FTP/SFTP profile、便携 ticket、接收后删除远端对象和 `ii doctor` 配置检查。
- 新增基于 Slint 的 `ii-gui` 桌面客户端，提供发送、接收、S3/WebDAV/TLS relay profile 管理、传输队列和诊断界面。

### 变更

- Release 流水线新增 Windows GUI 可执行文件、Linux AppImage 和 macOS `.app.zip` 产物。
- 裁剪 relay、S3、WebDAV、FTP 和日志依赖；保留现有 CLI、配置格式和传输协议兼容性，并在发布流水线中增加三端 CLI 的 UPX 完整性与 1 MiB 大小门禁。

### 文档

- 新增 FTP 与 SFTP 中转说明，并同步中英文 README 和完整命令手册。

## 0.1.12 - 2026-07-17

### 变更

- 更新发布版本元数据。

## 0.1.11 - 2026-07-17

### 新增

- `ii relay --public <https-url>` 自动生成并持久化自签 HTTPS relay 证书。
- `ii send --relay <https-url> -k` 接受自签 relay，并把该信任策略写入 ticket，接收端无需额外配置。

### 变更

- 指定 `--relay` 时发送和接收都强制 relay-only，不再尝试 UDP、局域网发现或点对点直连。
- 手工 TLS relay 继续使用系统证书验证；自签 relay 的首次连接仍可能遭遇中间人替换。

### 文档

- 更新自签 relay、手工 TLS、端口、状态文件和安全边界说明。

## 0.1.10 - 2026-07-17

### 变更

- `ii relay` 新增 `--tls <domain> --cert <path> --key <path>`，使用运维方提供的 PEM 证书和私钥启动 HTTPS relay。
- TLS 模式不再开放公网 HTTP relay；证书和域名必须由运维方负责。
- 移除 ACME 自动证书、自动续期和 QUIC 地址发现，默认 relay 保持纯 HTTP。
- `ii doctor` 默认检查 `3340/tcp`。

### 文档

- 更新 HTTPS 手工证书和 relay 端口说明。

## 0.1.9 - 2026-07-17

### 新增

- `ii relay` 默认启动无需域名或证书的 HTTP relay，监听 `3340/tcp`。

### 变更

- TLS、QUIC 地址发现和 metrics 改为通过 relay 配置显式启用，默认不启动。
- 移除默认 DNS peer discovery 和未使用的 CLI 依赖，缩小发布依赖树。

### 文档

- 更新 relay 的默认启动、HTTPS/QUIC 配置和反向代理说明。

## 0.1.8 - 2026-07-16

### 修复

- 修复 Windows 配置路径单元测试在 Linux/macOS runner 上解析反斜杠路径失败的问题。

## 0.1.7 - 2026-07-16

### 变更

- Release 构建启用 LTO、strip、`opt-level = "z"` 和 `panic = "abort"`，进一步压缩二进制体积。
- `ii doctor` 在未启用 `relay-metrics` feature 时明确显示 metrics 未启用。

### 修复

- 修正 S3/WebDAV 默认 profile 选择逻辑，避免被 `[storage].profile` 的旧共享字段互相干扰。
- 保留旧 `[storage.s3.cloudflare]` 配置的兼容迁移，默认 S3 profile 统一为 `default`。

## 0.1.6 - 2026-07-16

### 文档

- README 进阶用法补充 `ii send --s3` 的 S3/R2 中转示例。

## 0.1.5 - 2026-07-16

### 新增

- 新增 `ii send --webdav`，支持通过 WebDAV 中转发送文件、stdin 和目录。
- 新增 `ii send --webdav -p`，生成包含 WebDAV URL、用户名和密码的便携 ticket，方便接收端无配置接收。
- `ii send --webdav -d` 支持接收成功后由接收端尝试删除 WebDAV 远端对象。
- 新增 `ii send --profile <name>`，用于选择 S3 或 WebDAV 后端 profile。
- `ii doctor` 增加 WebDAV 配置检查。

## 0.1.4 - 2026-07-16

### 变更

- Windows Release 压缩改为使用仓库内置的 UPX 5.1.0，不再在 GitHub Actions 中临时下载 UPX。

## 0.1.3 - 2026-07-16

### 新增

- `ii recv` 在交互式终端中实时显示传输进度和传输速率。
- 新增 `ii send -c`，显式把 `ii recv ...` 接收命令复制到剪贴板。
- 新增 `ii send -o <path>`，把 `ii recv ...` 接收命令写入指定文件。
- `ii recv` 传输完成时显示总耗时和平均速度。

## 0.1.2 - 2026-07-15

### 变更

- 加入正式 `ii` logo 资源。
- README 顶部加入 logo 展示。
- Windows 构建时把 `logo.ico` 嵌入 `ii.exe`。

## 0.1.1 - 2026-07-15

### 变更

- GitHub Actions Release 产物改为直接发布原始二进制文件，不再打包成 zip 或 tar.gz。
- Windows Release 可执行文件保留 UPX 压缩。
- README 加入同事临时传文件场景截图。

## 0.1.0 - 2026-07-15

### 新增

- 新增 `ii` CLI，包含 `send`、`recv`、`relay`、`doctor`、`version`。
- 支持文件、文件夹和 stdin 传输。
- `ii send` 默认一次性发送；使用 `-t` 可以保持发送端继续运行，允许多个接收端接收。
- `ii recv` 默认支持断点续传、覆盖同名不同内容文件、跳过同名同内容文件。
- 新增 `ii relay`，支持 relay 配置生成和端口覆盖。
- 新增 `ii recv --trace`，用于输出连接和传输耗时诊断。

### 变更

- 目录接收结果改为只生成一层顶级目录，避免出现重复嵌套目录。
- 接收端连接策略改为先短时间尝试完整地址集，失败后回退到 relay-only，避免不可达地址拖慢建连。

### 修复

- 修复传输完成后连接关闭等待不完整导致的不完整传输问题。
- 修复发送端在成功接收后仍可能输出超时错误的问题。

### 破坏性变更

- 移除 `ii send --once`；一次性发送现在是默认行为。
- 新增 `ii send -t` 用于原来的保持运行行为。
