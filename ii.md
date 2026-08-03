# ii 用户手册

`ii` 是唯一对外品牌和唯一用户入口。用户只需要记 `ii`，不用记 `sendme`、`provide/get`、`iroh-relay`，也不用接触 `hash`、`peer id` 这些底层词。

## 一句话

`ii send` 发，`ii recv` 收，`ii web` 开局域网目录，`ii webrtc` 开浏览器直传，`ii tunnel` 转发 TCP，`ii relay` 管中继，`ii doctor` 查问题，`ii version` 看版本。

## 命令总览

```text
ii help [<command>]
ii send [<path>] [--name <name>] [-t] [-c] [-o <path>] [--web [--port <port>] [--token [<value>]] [--upload] [--path <dir>] | --s3 | --webdav | --ftp | --sftp] [--profile <name>] [-d] [-p] [--local] [--relay <url> [-k]] [--no-relay]
ii web [<目录>] [--port <port>] [--token [<value>]] [--upload] [--path <目录>]
ii webrtc [--port <port>] [--token [<value>]]
ii tunnel -s <target-host:port> [--relay <url> [-k]]
ii tunnel -c <ticket> [--listen <ip:port>]
ii recv <ticket> [-o <dir>] [--stdout] [--overwrite] [--resume] [--local] [--trace]
ii relay [--port <port>] [--tls [--domain <name>] [--cert <path> --key <path>]]
ii doctor
ii version
```

`ii help` 显示命令总览；`ii help send`、`ii help web`、`ii help webrtc`、`ii help tunnel`、`ii help recv`、`ii help relay`、`ii help doctor`、`ii help version` 显示对应命令帮助，内容与 `<command> --help` 相同。

## 核心规则

- 命令要直：`send` 就是发送，`recv` 就是接收。
- 用户只复制 `ticket`，不手工拼内部地址。
- 默认先走直连和局域网，必要时再走公网 relay。
- 需要显式限制路径时，用 `--local`、`--relay`、`--no-relay`。
- `ii relay` 是运维命令，不是用户日常发文件要记的东西。
- `--s3`、`--webdav`、`--ftp` 和 `--sftp` 是可选中转后端，第一次会初始化本机 `ii.toml`。

## `ii send`

### 用法

```powershell
ii send .\video.mp4
```

```powershell
ii send .\my-folder
```

```powershell
tar czf - .\project | ii send --name project.tar.gz
```

### 行为

- 发送文件或文件夹时，`ii send` 会生成一个 ticket。
- ticket 打出来后，发送端默认只成功发送一次，完成后自动退出。
- 如果需要保持运行、允许多个接收端继续取同一个 ticket，用 `-t`。
- 默认不会改剪贴板；需要复制接收命令时，用 `-c`。
- 需要把接收命令写到文件时，用 `-o <path>`。
- 默认发送路径是自动选择的：先直连，再局域网发现，再公网 relay。
- 如果直连/局域网能成，就不必碰公网 relay。
- ticket 是唯一需要传给另一台电脑的值。

### 参数

`<path>`
: 要发送的文件或文件夹。  
  如果不提供 `<path>`，且 stdin 不是交互终端，就进入 stdin 模式。

`--name <name>`
: 指定接收端看到的名字。stdin 模式必须配这个。  
  例子：

```powershell
tar czf - .\project | ii send --name project.tar.gz
```

`-t`
: 发送完成后不退出，继续保持 ticket 可用，直到用户 `Ctrl+C`。

`-c`
: 把完整的 `ii recv ...` 命令复制到系统剪贴板。  
  Windows 使用 `clip.exe`，macOS 使用 `pbcopy`，Linux 会依次尝试 `wl-copy`、`xclip`、`xsel`。

`-o <path>`
: 把完整的 `ii recv ...` 命令写到指定文件路径。
  如果文件已存在，会覆盖。这个 `-o` 属于 `ii send`，不影响 `ii recv -o <dir>` 的保存目录语义。

`--web`
: 在局域网内临时开放一个无账号鉴权 HTTP 下载页。执行后会在主 URL 上方展示进入下载页的二维码，随后在 `other:` 下列出其余物理和虚拟网卡的 IPv4 URL；下载页顶部二维码直达 `/download`。默认只提供下载；传入 `--upload` 后才开放多文件上传。按 `Ctrl+C` 停止服务。文件直接下载；文件夹会按原目录名打包为 `.tar` 下载。它不生成 ticket，不能和 `-c`、`-o`、`--s3`、`--webdav`、`--ftp`、`--sftp`、`--local`、`--relay`、`--no-relay` 同时使用。

`--port <port>`
: 仅和 `--web` 同用，指定 `1` 到 `65535` 的 HTTP 监听端口。未提供时由系统随机选择；`0`、非数字、超范围或已占用端口会报错。

`--token [value]`
: 仅和 `--web` 同用。裸 `--token` 自动生成 32 字符路径令牌，并把真实 URL 打印到终端；`--token <value>` 或 `--token=<value>` 使用指定令牌。令牌会把网页、下载和已启用的上传 URL 固定到 `/<value>/` 路径下；遗漏或写错路径会返回 `404`。显式 `value` 必须为 16 到 128 个 ASCII 字母、数字、`-` 或 `_`。不提供 `--token` 时仍使用原来的无令牌 URL。

`--upload`
: 仅和 `--web` 同用，显示网页多文件上传控件并开放上传接口。默认上传目录是启动命令当前目录的 `./ii/`，不支持上传目录。

`--path <dir>`
: 仅和 `--web --upload` 同用，指定网页上传文件直接写入的目录。相对路径以启动命令当前目录为基准；目录在首次上传时创建。不提供时仍写入当前目录的 `./ii/`。未提供 `--upload` 时本参数会被忽略。`-p` 仍是 FTP/SFTP/WebDAV 的便携 ticket 参数。

`--local`
: 只走局域网优先路径，不走公网发现，不走公网 relay。

`--relay <url>`
: 使用 HTTP 或 HTTPS relay-only 模式，URL 必须是 `http://主机[:端口]` 或 `https://主机[:端口]`。
  发送端和接收端都只通过该 relay 传输，不尝试 UDP、局域网发现或点对点直连。
  HTTPS 默认按系统证书链校验，适合 `ii relay --tls --cert --key` 的手工证书模式。

`-k`
: 只允许和 HTTPS `--relay` 一起使用，表示接受该 relay 的自签证书。
  用于 `ii relay --tls`。带 `-k` 的 ticket 会让接收端自动沿用自签信任策略。

`--no-relay`
: 禁用 relay，只允许直连和局域网路径。

`--s3`
: 走对象存储后端，不走 peer/relay 路径。默认 profile 是 `default`，默认 provider 是 Cloudflare R2。  
  如果本机还没有配置，`ii` 会在终端里依次提示 `Account ID`、`Bucket`、`Access Key ID`、`Secret Access Key`，成功后把配置写到平台默认路径：Windows 是 `ii.exe` 同目录下的 `ii.toml`，Linux/macOS/其他 Unix-like 是 `/etc/ii/ii.toml`。  
  之后再执行 `ii send ... --s3` 时，会直接复用这份配置。

`--profile <name>`
: 只在 `--s3`、`--webdav`、`--ftp` 或 `--sftp` 模式下生效，用来选择 `ii.toml` 里的后端 profile。
  例子：`ii send .\file.zip --s3 --profile work`、`ii send .\file.zip --webdav --profile nas`、`ii send .\file.zip --ftp --profile legacy`、`ii send .\file.zip --sftp --profile server`。
  四种后端不指定时都默认使用 `default`。旧的 `[storage.s3.cloudflare]` 会自动兼容迁移为 S3 的 `default` profile。

`-d`
: 只在 `--s3`、`--webdav`、`--ftp` 或 `--sftp` 模式下生效。接收端拿到文件后，会尝试删除中转端里的对应对象；删除失败会忽略，不影响下载结果。

`--webdav`
: 走 WebDAV 中转后端，不走 peer/relay 路径。  
  如果本机还没有配置，`ii` 会在终端里依次提示 `URL`、`Username`、`Password`，三项都是明文输入。上传成功后把配置写到平台默认路径：Windows 是 `ii.exe` 同目录下的 `ii.toml`，Linux/macOS/其他 Unix-like 是 `/etc/ii/ii.toml`。  
  文件和 stdin 会按 `remote_dir/<md5>` 存到 WebDAV；同 MD5 对象已存在时不重复上传。

`--ftp`
: 走 FTP 中转后端，不走 peer/relay 路径。首次缺配置时，`ii` 提示 `FTP URL`、`Username`、`Password`，上传成功后才写入 `ii.toml`。只接受明文 `ftp://主机[:端口]`；账号、文件和控制命令都可能被网络上的人读取。详见 [ftp.md](ftp.md)。

`--sftp`
: 走 SFTP 中转后端，不走 peer/relay 路径。首次缺配置时，`ii` 提示主机、用户名和密码或私钥路径，上传成功后才写入 `ii.toml`。支持密码与 SSH 私钥认证。每次连接都会打印并直接接受服务器 SSH SHA-256 主机指纹，不保存 known-hosts；服务器仍可被中间人替换。详见 [sftp.md](sftp.md)。

`-p`
: 只在 `--webdav`、`--ftp` 或 `--sftp` 模式下生效。生成便携 ticket。WebDAV/FTP ticket 写入 URL、用户名和密码；SFTP 密码 ticket 写入密码，私钥 ticket 写入私钥文本和口令。
  ticket 只有编码，没有加密。谁拿到 ticket 谁就拿到了这次后端访问凭据。接收成功后，配置会写入本机 `ii.toml`；便携 SFTP 私钥会另存为密钥文件，配置只保存路径。

### 路径规则

- `--web`、`--s3`、`--webdav`、`--ftp`、`--sftp`、`--local`、`--relay`、`--no-relay` 互斥。
- 默认不需要用户选 relay。
- 如果没有局域网或直连可用，默认会自动退到公网 relay。
- 指定 `--relay http://...` 或 `--relay https://...` 后，当前发送会强制走 relay-only，不使用默认公网 relay。
- 手工证书 relay 不带 `-k`；自签 relay 必须带 `-k`。

## `ii web`

```powershell
ii web
ii web .\shared
ii web .\shared --port 8080 --token A1b2C3d4E5f6G7h8 --upload --path .\uploads
```

`ii web` 不带路径时服务启动命令当前目录；带路径时必须是已有目录，文件路径会报错。网页默认提供 nginx 风格的递归目录浏览：目录可继续进入、`../` 返回父目录、每项显示名称、修改时间和大小；普通文件直接响应，不强制下载。传入 `--upload` 后网页顶部才显示多文件上传控件，不支持上传目录。

命令行会在主 IPv4 LAN URL 上方打印根页二维码，并在 `other:` 下打印其他物理和虚拟网卡 URL；网页不显示二维码。按 `Ctrl+C` 关闭服务。

`--port <port>` 指定 `1` 到 `65535` 的 HTTP 监听端口，未提供时随机选择。裸 `--token` 自动生成 32 字符路径令牌；`--token <value>` 或 `--token=<value>` 使用指定令牌，把目录、文件和已启用的上传 URL 固定到 `/<value>/` 下；遗漏或写错返回 `404`。显式令牌必须为 16 到 128 个 ASCII 字母、数字、`-` 或 `_`。`--upload` 才显示上传控件并开放上传接口；`--path <目录>` 指定上传文件直接写入的目录，相对路径按启动目录解析，首次上传才创建目录，未指定时写入启动目录下的 `./ii/`，未提供 `--upload` 时会被忽略。`-p` 不适用于 `ii web`，仍仅用于 WebDAV、FTP、SFTP 的便携 ticket。

## `ii webrtc`

```powershell
ii webrtc
ii webrtc --port 8080 --token A1b2C3d4E5f6G7h8
```

`ii webrtc` 在 `0.0.0.0` 的 HTTP 端口提供局域网页面；`--port <port>` 可指定 `1` 到 `65535`，未提供时随机选择。命令行在主 IPv4 LAN URL 上方打印二维码，并在 `other:` 下打印其他物理和虚拟网卡 URL；按 `Ctrl+C` 关闭服务。打开同一 URL 的浏览器自动成为临时编号设备，可以选择任一在线设备并发送多个独立文件或文本消息；文本只发送给当前选中设备，接收端在当前页面列表中显示完整消息并可逐条复制，对方文件仍自动接收并下载。

文本输入使用 UTF-8 字节传输，分片不超过 1 MiB，接收端重组为一条消息，不显示分片。接收消息列表只存在于当前页面，刷新后清空；`ii` 不保存消息，也不建立聊天记录。

`ii` 只在 HTTP 上转发 WebRTC 信令，不接收文件，不写入本机磁盘；文件通过浏览器的可靠、有序 DataChannel 点对点传输。页面加入房间前会实际测试浏览器能否创建 ICE host candidate；浏览器禁用或阻止 WebRTC 时会显示 `WebRTC unavailable`，不会加入房间。它只使用局域网 host candidates，不使用公网 STUN/TURN，也不支持目录、断点续传或 CLI 与浏览器直接传输。访客网络隔离、防火墙、跨网段限制或禁用 P2P 时会无法连接。接收浏览器会把单个文件聚合到内存后再下载，大文件会占用相近大小的内存。完整说明见 [webrtc.md](webrtc.md)。

不带 `--token` 时使用根路径；裸 `--token` 自动生成 32 字符路径令牌，`--token <value>` 或 `--token=<value>` 将页面和信令接口固定到 `/<value>/` 下，遗漏或写错返回 `404`。显式令牌必须为 16 到 128 个 ASCII 字母、数字、`-` 或 `_`。`ii webrtc` 不接受路径、`--path` 或 `-p`。

## `ii tunnel`

```powershell
# A: A 可访问目标 TCP 服务
ii tunnel -s 127.0.0.1:22

# B: 使用 A 输出的 ticket
ii tunnel -c ii1k7v...x9a
```

`-s <target-host:port>` 在 A 上持续等待连接。目标可以是 A 本机服务、A 所在局域网的 NAS，或 A 能解析的 DNS 主机；目标地址不会写入 ticket。A 会输出 `ii tunnel -c <ticket>`，B 执行它后，本机 TCP 客户端连接 B 的监听地址即可通过加密 Iroh 连接访问 A 的目标服务。每条本地 TCP 连接对应一条 Iroh 双向 stream，同时最多转发 64 条。

`-c <ticket>` 默认监听 `127.0.0.1:8080`；端口不可用时按顺序尝试 `8081` 至 `65535`，并打印实际监听地址。`--listen <ip:port>` 明确指定监听地址，失败直接报错，不自动换端口。默认只允许 B 本机访问；需要让 B 所在局域网设备接入时，显式使用 `--listen 0.0.0.0:端口`。

默认先尝试直连和局域网路径，必要时使用 Iroh 默认 relay。指定 relay 时只在 A 上使用：

```powershell
ii relay --port 8443
ii tunnel -s 192.168.1.10:5000 --relay http://公网IP:8443
```

`--relay <url>` 可为 `http://主机[:端口]` 或 `https://主机[:端口]`，使本次 tunnel 强制走该 relay；`-k` 仅和 HTTPS `-s --relay` 同用，接受自签 relay。ticket 会把 relay URL 和自签信任策略带给 B，B 不需要再次配置。relay 只转发加密 Iroh 流量，不会把目标 TCP 端口直接暴露到公网。

ticket 内有一次随机访问密钥。持有 ticket 的设备可以接入，直到 A 按 `Ctrl+C` 停止 tunnel；不要泄露 ticket。首版不支持 UDP、SOCKS、反向 tunnel 或后台守护。

## `ii recv`

### 用法

```powershell
ii recv ii1k7v...x9a
```

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

```powershell
ii recv ii1k7v...x9a --stdout > project.tar.gz
```

```powershell
ii recv ii1k7v...x9a --trace
```

```powershell
ii recv ii1k7v...x9a --local --trace
```

### 行为

- `ii recv` 只需要 ticket。
- 默认把内容写到当前目录。
- 默认智能处理同名文件：完整重复就跳过，未完成就续传，内容不同就覆盖。
- 如果 ticket 对应的是文件夹，按目录结构还原。
- `--stdout` 只适合单文件或流式内容，不适合目录。
- 文件和 stdin 字节流默认自带断点续传，不需要手工加 `--resume`。
- `ii send` 和 `ii recv` 都会在终端里实时显示进度和速率，完成后打印最终耗时；`--trace` 主要用于诊断，不建议和正常进度条混着看。

### 参数

`<ticket>`
: 从发送端复制来的 ticket。

`-o <dir>`
: 指定保存目录。

`--stdout`
: 把内容写到标准输出，适合管道和重定向。

`--overwrite`
: 强制从头覆盖目标路径里已有的同名文件。通常不需要手工使用。

`--resume`
: 强制按已有文件大小续传。通常不需要手工使用，因为默认会自动判断。

`--local`
: 只走局域网优先路径，不碰公网 relay。

`--trace`
: 输出接收过程的分段耗时、地址统计、写入字节数和平均速度，便于排查为什么慢。

### 接收规则

- `--stdout` 和 `--resume` 不同时用。
- `--local` 只影响路径选择，不影响 ticket 本身。
- recv 不需要用户知道发送端用了哪条路；它只按 ticket 和可用网络路径工作。
- 对文件和 stdin 字节流，默认顺序是：目标不存在就下载；目标更短就续传；目标同名同尺寸且 MD5 一致就跳过；同名但内容不同就覆盖。
- 文件夹继续可传输，重复运行时会重新解包到目标目录；目录不做 MD5 去重。
- 默认模式下，如果 ticket 同时带 relay 和很多直连地址，`ii recv` 会先给完整地址集一个短直连窗口；短时间内连不上就切到 relay-only，避免不可达的私网/VPN 地址把建连拖到十几秒。
- relay-only ticket 自带 relay 地址和 TLS 策略；接收端无需安装证书或写 relay 配置，也不会尝试 UDP 或直连。自签 ticket 只由发送端带 `-k` 时生成。
- 排查慢的时候，先跑一次默认模式，再跑一次 `--local` 对比；如果 `--local` 明显快，问题通常在公网发现或 relay 路径，不在本地写盘。
- WebDAV、FTP 和 SFTP 普通 ticket 不带凭据，接收端首次使用对应 profile 时会提示补齐配置，下载成功后保存到 `ii.toml`。三种 `-p` ticket 都会直接使用 ticket 内的凭据并在成功接收后保存本机配置；便携 SFTP 私钥会另存为密钥文件。

## ticket

ticket 是用户层唯一交换物，格式以 `ii` 开头。

```text
ii1k7v...x9a
```

ticket 里可以带足够完成连接、恢复传输和重复文件判定的最小信息，但用户不直接操作这些底层字段。

用户层只认：

- 复制 ticket
- 贴到另一台电脑上执行 `ii recv`

不要求用户接触：

- blob hash
- peer id
- token
- endpoint
- 文件内容指纹

例外：`ii send --webdav -p`、`ii send --ftp -p`、`ii send --sftp -p` 会把后端访问凭据放进 ticket，让没有本机配置的接收方也能直接 `ii recv`。ticket 只有编码，没有加密；SFTP 还会直接接受服务器主机指纹。

## 中继规则

### 默认规则

默认路径选择顺序是：

1. 直连
2. 局域网发现
3. 公网 relay

也就是说，`ii send` 和 `ii recv` 默认都不需要用户先想“我该连哪个中继”。

### `--local`

`--local` 的意思是：

- 只用局域网发现
- 不用公网发现
- 不用公网 relay

适合同一局域网内的机器互传。

### `--relay <url>`

`--relay` 的意思是：

- 强制指定某个 relay
- 不按默认 relay 列表自动挑

### `--no-relay`

`--no-relay` 的意思是：

- 不走公网 relay
- 只靠直连和局域网路径

## `ii relay`

`ii relay` 默认启动 HTTP relay，不需要参数：

```powershell
ii relay
ii relay --port 8443
```

它监听 `0.0.0.0:随机端口`，或监听 `--port` 指定的 `0.0.0.0:端口`。终端按 `ii web` 的格式输出主 IPv4 URL 和 `other:` 下的其余物理、虚拟网卡 IPv4 URL，但不显示二维码。`0.0.0.0` 只是 bind 地址，不能作为客户端 URL；客户端必须使用实际可达 IP 或域名。

裸机或公网 IP 直接绑在网卡时，打印列表会包含公网地址。云服务器常由 NAT 映射公网 IP，网卡列表只有私网地址；这时从云控制台取得公网 IP，再与终端打印的端口拼成 relay URL。`ii` 不尝试猜测 NAT 映射。

HTTP relay 示例：

```powershell
ii relay --port 8443
ii send .\video.mp4 --relay http://公网IP:8443
ii tunnel -s 192.168.1.10:5000 --relay http://公网IP:8443
```

### TLS

`--tls` 开启 HTTPS。没有 `--cert` 和 `--key` 时，`ii` 仅为当前进程生成自签证书；客户端必须带 `-k`：

```powershell
ii relay --tls --port 8443
ii send .\video.mp4 --relay https://公网IP:8443 -k
```

需要以域名访问时：

```powershell
ii relay --tls --domain relay.example.com --port 8443
ii send .\video.mp4 --relay https://relay.example.com:8443 -k
```

带 `--domain` 时，终端只输出该域名 HTTPS URL；自动证书包含该 DNS SAN。没有 `--domain` 时，终端输出 HTTPS 网卡 IP URL，仍需 `-k`。

已有 PEM 完整证书链和私钥时，成对提供 `--cert`、`--key` 替换自动证书：

```powershell
ii relay --tls --domain relay.example.com --port 8443 --cert .\fullchain.pem --key .\privkey.pem
ii send .\video.mp4 --relay https://relay.example.com:8443
```

手工证书模式通常配合 `--domain`，客户端正常校验证书，不带 `-k`。允许不带 `--domain` 使用手工证书；此时终端输出 HTTPS 网卡 IP URL，证书必须包含对应 IP SAN，否则用户需要自行改用匹配证书名或 `-k`。

`--domain` 只能与 `--tls` 同用；`--cert` 和 `--key` 必须成对出现，且也要求 `--tls`。旧 `--public` 与 `-H` 已删除。HTTP 或 HTTPS 的 `--relay` 都强制 relay-only：不尝试局域网发现、UDP 打洞或点对点直连。

### 安全边界

HTTP relay 不提供 relay 连接层 TLS；HTTPS 自签模式的 `-k` 会跳过证书校验，首次连接可能被中间人替换 relay。Iroh 的端到端节点认证仍在；手工证书模式不带 `-k`，继续使用系统 TLS 证书校验。

### 日志

启动后会输出可访问 URL、客户端连接和断开日志。需要更详细的协议日志时设置 `RUST_LOG`，例如：

```powershell
$env:RUST_LOG="debug"
ii relay --port 8443
```

## `ii doctor`

```powershell
ii doctor
```

`doctor` 用来查：

- 网络连通性
- 直连是否可用
- 局域网发现是否可用
- relay 是否可用
- 端口和权限问题
- 版本和运行环境

## `ii version`

```powershell
ii version
```

输出当前 `ii` 版本。

## 底层对应关系

这部分只做对照，不进用户主路径。

- `ii send` / `ii recv`：`iroh-blobs`
- ticket：`iroh-tickets`
- 局域网发现：`iroh-mdns-address-lookup`
- 公网发现、NAT 穿透、relay：`iroh`
- relay 服务：`iroh-relay`
- S3 中转：`rust-s3`
- WebDAV 中转：`reqwest_dav`
- FTP 中转：`suppaftp`
- SFTP 中转：`russh`、`russh-sftp`

## 源码对照

- [iroh-relay/src/main.rs](https://github.com/n0-computer/iroh/blob/main/iroh-relay/src/main.rs)
- [iroh-relay/src/server.rs](https://github.com/n0-computer/iroh/blob/main/iroh-relay/src/server.rs)
- [Iroh Docs: Add a relay](https://docs.iroh.computer/add-a-relay)
