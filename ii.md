# ii 用户手册

`ii` 是唯一对外品牌和唯一用户入口。用户只需要记 `ii`，不用记 `sendme`、`provide/get`、`iroh-relay`，也不用接触 `hash`、`peer id` 这些底层词。

## 一句话

`ii send` 发，`ii recv` 收，`ii web` 开局域网目录，`ii dav` 开 WebDAV，`ii ftp` 开 FTP 服务，`ii http`、`ii paste`、`ii drop`、`ii pac`、`ii speed` 开轻量 LAN 服务，`ii proxy`、`ii tcp`、`ii udp` 开代理或转发，`ii ping`、`ii port`、`ii health`、`ii wake` 查网络或唤醒设备，`ii discover` 找本地服务，`ii webrtc` 开浏览器直传，`ii tunnel` 转发 TCP，`ii socks5` 开普通代理，`ii relay` 管中继，`ii doctor` 查问题，`ii version` 看版本。

## 命令总览

```text
ii help [<command>]
ii send [<path>...] [--name <name>] [--include <glob>] [--exclude <glob>] [--rate <bytes/s>] [--json] [--checksum <md5|sha256>] [--preserve-metadata] [--symlinks <follow|preserve|reject>] [--quic-port <port>] [-t] [-c] [-o <path>] [--web [--port <port>] [--bind <ip>] [--token [<value>]] [--upload] [--path <path>] | --s3 | --r2 | --azure | --webdav | --ftp | --sftp] [--profile <name>] [-d] [-p] [--local] [--relay <url> [-k]] [--no-relay]
ii watch <目录> [--interval <duration>] [--stabilize <duration>] [发送选项]
ii queue <path...> [--after <duration>|--every <duration>] [发送选项]
ii web [<目录>] [--port <port>] [--bind <ip>] [--token [<value>]] [--upload] [--path <目录>] [--once]
ii dav [<目录>] [--port <port>] [--bind <ip>] [--token [<value>]] [--read-only] [--username <username> --password <password>] [--tls [--domain <name>] [--cert <path> --key <path>]]
ii socks5 [--port <port>] [--bind <ip>] [--username <user> --password <pass>]
ii http [<目录>] [--port <port>] [--bind <ip>] [--token [<value>]]
ii paste [<text>] [--port <port>] [--bind <ip>] [--token [<value>]] [--ttl <duration>]
ii drop [<目录>] [--port <port>] [--bind <ip>] [--token [<value>]]
ii ftp [<目录>] [--bind <ip>] [--port <port>] [--username <username> --password <password>] [--rate <bytes/s>] [--max <n>] [--upload <true|false>] [--download <true|false>] [--delete <true|false>] [--rename <true|false>] [--mkdir <true|false>] [--tls [--implicit] [--cert <path> --key <path>]] [--passive-host <IPv4|hostname>] [--passive-ports [start-end]]
ii proxy [--port <port>] [--bind <ip>] [--username <user> --password <pass>]
ii tcp <host:port> [--port <port>] [--bind <ip>]
ii udp <host:port> [--port <port>] [--bind <ip>]
ii ping <host:port> [--count <n>] [--interval <duration>] [--timeout <duration>]
ii speed serve [--port <port>] [--bind <ip>] [--token [<value>]]
ii speed <http-url> [--duration <duration>]
ii wake <mac> [--broadcast <ip>] [--port <port>]
ii port <host> <port...> [--timeout <duration>]
ii health <http-url|host:port> [--interval <duration>] [--timeout <duration>]
ii pac --proxy <http://host:port|socks5://host:port> [--port <port>] [--bind <ip>] [--token [<value>]]
ii webrtc [--port <port>] [--bind <ip>] [--token [<value>]]
ii tunnel -s <target-host:port> [--relay <url> [-k]]
ii tunnel -c <ticket> [--listen <ip:port>]
ii recv <ticket> [-o <dir>] [--stdout] [--overwrite] [--resume] [--local] [--trace] [--checksum <md5|sha256>] [--quic-port <port>]
ii relay [--port <port>] [--tls [--domain <name>] [--cert <path> --key <path>]]
ii discover [--json]
ii doctor [--nat]
ii version
```

`ii help` 显示命令总览；`ii help <command>` 可显示上述每个命令的对应帮助，内容与 `<command> --help` 相同。

## 核心规则

- 命令要直：`send` 就是发送，`recv` 就是接收。
- 用户只复制 `ticket`，不手工拼内部地址。
- 默认先走直连和局域网，必要时再走公网 relay。
- 需要显式限制路径时，用 `--local`、`--relay`、`--no-relay`。
- `ii relay` 是运维命令，不是用户日常发文件要记的东西。
- `--s3`、`--r2`、`--azure`、`--webdav`、`--ftp` 和 `--sftp` 是可选中转后端，第一次会初始化本机 `ii.toml`。

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
- 如果需要保持运行、允许多个接收端继续取同一个 ticket，用 `-t`；最多同时传给 16 个接收端，额外最多 1000 个连接按先到先服务排队。
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
: 保持 ticket 可用，直到用户 `Ctrl+C`。最多 16 个接收端并发传输，额外最多 1000 个连接按先到先服务排队；并发接收会共享发送端带宽，队列满时接收端需要稍后重试。普通 `ii send` 仍只在首次成功发送后退出。在 `--web` 模式下，`-t` 让下载页持续服务直到 `Ctrl+C`。

`-c`
: 把完整的 `ii recv ...` 命令复制到系统剪贴板。  
  Windows 使用 `clip.exe`，macOS 使用 `pbcopy`，Linux 会依次尝试 `wl-copy`、`xclip`、`xsel`。

`-o <path>`
: 把完整的 `ii recv ...` 命令写到指定文件路径。
  如果文件已存在，会覆盖。这个 `-o` 属于 `ii send`，不影响 `ii recv -o <dir>` 的保存目录语义。

`--web`
: 在局域网内临时开放一个无账号鉴权 HTTP 下载页。执行后会在主 URL 上方展示进入下载页的二维码，随后在 `other:` 下列出其余物理和虚拟网卡的 IPv4 URL；下载页顶部二维码直达 `/download`。默认只提供下载；传入 `--upload` 后才开放多文件上传。默认在第一个 `/download` 完整 HTTP 响应体写完并成功关闭连接后立即退出；`-t` 才持续服务到 `Ctrl+C`。并发下载时，第一个完成会直接结束进程；访问下载页、上传和失败下载不会触发退出。文件直接下载；文件夹会按原目录名打包为 `.tar` 下载。它不生成 ticket，不能和 `-c`、`-o`、`--s3`、`--r2`、`--azure`、`--webdav`、`--ftp`、`--sftp`、`--local`、`--relay`、`--no-relay` 同时使用。

`--port <port>`
: 仅和 `--web` 同用，指定 `1` 到 `65535` 的 HTTP 监听端口。未提供时由系统随机选择；`0`、非数字、超范围或已占用端口会报错。

`--token [value]`
: 仅和 `--web` 同用。裸 `--token` 自动生成 32 字符路径令牌，并把真实 URL 打印到终端；`--token <value>` 或 `--token=<value>` 使用指定令牌。令牌会把网页、下载和已启用的上传 URL 固定到 `/<value>/` 路径下；遗漏或写错路径会返回 `404`。显式 `value` 必须为 16 到 128 个 ASCII 字母、数字、`-` 或 `_`。不提供 `--token` 时仍使用原来的无令牌 URL。

`--upload`
: 仅和 `--web` 同用，显示网页多文件上传控件并开放上传接口。默认上传目录是启动命令当前目录的 `./ii/`，不支持上传目录。浏览器按 1 MiB 分块流式写入；断网或刷新后重新选择同一文件会继续未完成上传。

`--path <dir>`
: 仅和 `--web --upload` 同用，指定网页上传文件直接写入的目录。相对路径以启动命令当前目录为基准；目录在首次上传时创建。不提供时仍写入当前目录的 `./ii/`。未提供 `--upload` 时本参数会被忽略。`-p` 仍是 FTP/SFTP/WebDAV 的便携 ticket 参数。

`<path>...`
: 可一次发送多个文件或文件夹。多个输入使用现有目录 ticket 打包到集合根目录，默认根名为 `ii`，可用 `--name` 覆盖；stdin 不能和路径混用，顶层名称重复会报错。

`--include <glob>` / `--exclude <glob>`
: 可重复使用，仅作用于目录和多路径归档。glob 按每个输入项内部的 `/` 相对路径匹配，exclude 永远优先；筛选后为空会报错。

`--rate <bytes/s>`
: 限制发送端总带宽，支持正整数 bytes/s 以及 `KiB`、`MiB`、`GiB` 后缀；多接收端共享同一个上限。

`--checksum md5|sha256`
: 只在本地计算实际发送字节并输出，不写入 ticket，也不自动比较。目录和多文件输出 tar 流校验和。

`--preserve-metadata`
: 只允许一个常规文件；将其包装为现有 tar 载荷，保留 mtime、权限和只读属性。它不支持 stdin、`--web`、断点续传或 MD5 秒传。

`--symlinks <follow|preserve|reject>`
: 目录和多路径归档中的符号链接策略，默认 `follow`；`preserve` 保留链接条目，`reject` 发现链接即失败。`watch` 和 `queue` 也接受此参数。

`--quic-port <1..65535>`
: 固定 P2P Iroh UDP 端口；未指定时仍随机。端口占用或无法绑定直接失败。

`--json`
: 输出稳定 JSON Lines；stdout 只输出事件，诊断和错误走 stderr。`recv --stdout --json` 不允许使用；JSON 后端缺 profile 时直接失败，不弹配置提示。

`--bind <ip>`
: 仅和 `--web` 同用，固定监听 IPv4 或 IPv6 地址。未提供时保持 `0.0.0.0` 并打印主 LAN URL 与 `other:`；显式地址只打印该地址，IPv6 URL 使用方括号。

`--local`
: 只走局域网优先路径，不走公网发现，不走公网 relay。

`--relay <url>`
: 使用 HTTP 或 HTTPS relay-only 模式，URL 必须是 `http://主机[:端口]` 或 `https://主机[:端口]`。可重复指定多个显式 relay；两个及以上时发送端先探测可达性和延迟，选择最快可达节点。
  发送端和接收端都只通过选中的 relay 传输，不尝试 UDP、局域网发现或点对点直连。
  HTTPS 默认按系统证书链校验，适合 `ii relay --tls --cert --key` 的手工证书模式。

`-k`
: 只允许和 HTTPS `--relay` 一起使用，表示接受该 relay 的自签证书。
  用于 `ii relay --tls`。带 `-k` 的 ticket 会让接收端自动沿用自签信任策略。

`--no-relay`
: 禁用 relay，只允许直连和局域网路径。

`--s3`
: 走通用 S3 兼容对象存储，不走 peer/relay 路径。默认 profile 是 `default`；首次使用依次配置 endpoint、region、bucket、access key、secret 和 path-style。path-style 默认开启，可改为虚拟主机风格。上传、HEAD 同 MD5 去重、流式 PUT、预签名 GET/DELETE 和 Range 续传都使用 S3 兼容 API。

`--r2`
: 走 Cloudflare R2，不走 peer/relay 路径。默认 profile 是 `default`；首次使用配置 Account ID、bucket、access key 和 secret。endpoint 固定推导为 R2 S3 API endpoint，region 固定为 `auto`，path-style 固定开启。R2 与通用 S3 使用独立配置段。

`--azure`
: 走 Azure Block Blob，不走 peer/relay 路径。默认 profile 是 `default`；支持 `auth = "shared-key"` 或 `auth = "sas"`。Shared Key 会生成对象级 GET SAS，使用 `-d` 时再生成对象级 DELETE SAS；SAS 模式要求 Container SAS 具有 `r`、`w` 权限，`-d` 还要求 `d`。SAS 会完整写入 ticket，ticket 泄露者得到 SAS 本身的全部权限范围。

`--profile <name>`
: 只在 `--s3`、`--r2`、`--azure`、`--webdav`、`--ftp` 或 `--sftp` 模式下生效，用来选择 `ii.toml` 里的后端 profile。
  例子：`ii send .\file.zip --s3 --profile work`、`ii send .\file.zip --r2 --profile r2`、`ii send .\file.zip --azure --profile blob`、`ii send .\file.zip --webdav --profile nas`。六种后端不指定时都默认使用 `default`。旧 S3 profile 若 `provider = "cloudflare-r2"` 会明确报错，必须手工写到 `[storage.r2.<name>]` 并改用 `--r2`；不会自动迁移或改写旧配置。

`-d`
: 只在 `--s3`、`--r2`、`--azure`、`--webdav`、`--ftp` 或 `--sftp` 模式下生效。接收端拿到文件后，会尝试删除中转端里的对应对象；删除失败会忽略，不影响下载结果。

`--webdav`
: 走 WebDAV 中转后端，不走 peer/relay 路径。  
  如果本机还没有配置，`ii` 会在终端里依次提示 `URL`、`Username`、`Password`，三项都是明文输入。上传成功后把配置写到平台默认路径：Windows 是 `ii.exe` 同目录下的 `ii.toml`，Linux/macOS/其他 Unix-like 是 `/etc/ii/ii.toml`。  
  文件和 stdin 会按 `remote_dir/<md5>` 存到 WebDAV；同 MD5 对象已存在时不重复上传。

`--ftp`
: 走 FTP 中转后端，不走 peer/relay 路径。首次缺配置时，`ii` 提示 `FTP URL`、`Username`、`Password`，上传成功后才写入 `ii.toml`。只接受明文 `ftp://主机[:端口]`；账号、文件和控制命令都可能被网络上的人读取。详见 [ftp.md](ftp.md)。

`--sftp`
: 走 SFTP 中转后端，不走 peer/relay 路径。首次缺配置时，`ii` 提示主机、用户名和密码或私钥路径，上传成功后才写入 `ii.toml`。支持密码与 SSH 私钥认证。每次连接都会打印并直接接受服务器 SSH SHA-256 主机指纹，不保存 known-hosts；服务器仍可被中间人替换。详见 [sftp.md](sftp.md)。

`SMB/NFS`
: 不提供原生 SMB/NFS 中转。SMB 的认证、签名、加密和 dialect 兼容，以及 NFS 的 RPC、认证、端口发现和版本兼容，不适合当前 Windows/Linux/macOS 完整兼容和小包体目标。系统已挂载的 SMB/NFS 目录仍可直接作为本地 `ii send` 输入。

`-p`
: 只在 `--webdav`、`--ftp` 或 `--sftp` 模式下生效。生成便携 ticket。WebDAV/FTP ticket 写入 URL、用户名和密码；SFTP 密码 ticket 写入密码，私钥 ticket 写入私钥文本和口令。
  ticket 只有编码，没有加密。谁拿到 ticket 谁就拿到了这次后端访问凭据。接收成功后，配置会写入本机 `ii.toml`；便携 SFTP 私钥会另存为密钥文件，配置只保存路径。

### 对象存储配置

Windows 配置文件在 `ii.exe` 同目录的 `ii.toml`；Linux、macOS 和其他 Unix-like 在 `/etc/ii/ii.toml`。三个对象存储后端各自使用独立 profile：

```toml
[storage.s3.default]
endpoint = "https://s3.example.com"
region = "us-east-1"
bucket = "files"
access_key_id = "..."
secret_access_key = "..."
path_style = true
prefix = "ii/"
presign_ttl_seconds = 86400
```

```toml
[storage.r2.default]
account_id = "..."
bucket = "files"
access_key_id = "..."
secret_access_key = "..."
prefix = "ii/"
presign_ttl_seconds = 86400
```

```toml
[storage.azure.default]
auth = "shared-key"
account_name = "..."
container = "files"
account_key = "..."
# endpoint = "https://<account>.blob.core.windows.net"
prefix = "ii/"
presign_ttl_seconds = 86400
```

Azure SAS 模式把 `auth` 改为 `"sas"`，移除 `account_key`，加入具有 `sr=c`、`sp=rw` 的 `sas_token`；使用 `-d` 时还需要 `sp` 包含 `d`。自定义 `endpoint` 可用于 Azure China、Azure Stack 或 Azurite。

### 路径规则

- `--web`、`--s3`、`--r2`、`--azure`、`--webdav`、`--ftp`、`--sftp`、`--local`、`--relay`、`--no-relay` 互斥。
- 默认不需要用户选 relay。
- 如果没有局域网或直连可用，默认会自动退到公网 relay。
- 指定 `--relay http://...` 或 `--relay https://...` 后，当前发送会强制走 relay-only，不使用默认公网 relay。
- 手工证书 relay 不带 `-k`；自签 relay 必须带 `-k`。

## `ii queue`

```powershell
ii queue .\a.zip .\b.zip
ii queue .\report.pdf --after 10s
ii queue .\folder --every 1h
```

每个位置参数是一个独立发送任务，按 FIFO 顺序执行，不会像 `ii send a b` 那样打成一个集合。`--after` 延迟一次执行；`--every` 在上一轮全部完成后等待再重复，二者互斥。duration 只接受正整数和 `ms`、`s`、`m`、`h` 后缀。队列只存在于当前进程，失败任务会报错后继续；`Ctrl+C` 会停止当前队列。

`queue` 接受 `--rate`、后端、relay、`--local`、`--no-relay`、`--checksum`、`--preserve-metadata`、`--symlinks` 和 `--quic-port`；拒绝 `-t`、`-c`、`-o`、`--web` 和 `--json`。

## `ii watch`

```powershell
ii watch .\incoming
ii watch .\incoming --interval 500ms --stabilize 2s --checksum sha256
```

`watch` 递归扫描普通文件，默认每 2 秒扫描；启动时已有文件只建立基线，不会立即发送。启动后的新增或变化文件在连续扫描中保持路径、大小和 mtime 不变达到 `--stabilize` 后入队，逐个生成独立 ticket 并按 FIFO 等待任务完成。它不使用文件系统监听器，进程退出后队列丢失。

`watch` 接受 `--rate`、后端、relay、`--local`、`--no-relay`、`--checksum`、`--preserve-metadata`、`--symlinks` 和 `--quic-port`；拒绝 `-t`、`-c`、`-o`、`--web` 和 `--json`。

## `ii web`

```powershell
ii web
ii web .\shared
ii web .\shared --port 8080 --token A1b2C3d4E5f6G7h8 --upload --path .\uploads
```

`ii web` 不带路径时服务启动命令当前目录；带路径时必须是已有目录，文件路径会报错。网页默认提供 nginx 风格的递归目录浏览：目录可继续进入、`../` 返回父目录、每项显示名称、修改时间和大小；普通文件直接响应，不强制下载。传入 `--upload` 后网页顶部才显示多文件上传控件，不支持上传目录；浏览器按 1 MiB 分块上传，刷新或断网后重新选择同一文件即可继续。

命令行会在主 IPv4 LAN URL 上方打印根页二维码，并在 `other:` 下打印其他物理和虚拟网卡 URL；网页不显示二维码。`--bind <ip>` 可固定 IPv4 或 IPv6 listener；显式 bind 只打印该地址。按 `Ctrl+C` 关闭服务。

`--once` 只在第一次完整的普通文件 `GET 200` 后关闭服务；HEAD、Range、目录页、404 和上传不会消耗单次机会，且不能与 `--upload` 同用。

`--port <port>` 指定 `1` 到 `65535` 的 HTTP 监听端口，未提供时随机选择。裸 `--token` 自动生成 32 字符路径令牌；`--token <value>` 或 `--token=<value>` 使用指定令牌，把目录、文件和已启用的上传 URL 固定到 `/<value>/` 下；遗漏或写错返回 `404`。显式令牌必须为 16 到 128 个 ASCII 字母、数字、`-` 或 `_`。`--upload` 才显示上传控件并开放上传接口；`--path <目录>` 指定上传文件直接写入的目录，相对路径按启动目录解析，首次上传才创建目录，未指定时写入启动目录下的 `./ii/`，未提供 `--upload` 时会被忽略。`-p` 不适用于 `ii web`，仍仅用于 WebDAV、FTP、SFTP 的便携 ticket。

## 轻量局域网服务

```powershell
ii http .\public
ii paste "meeting code: 123456" --ttl 30m
Get-Content .\note.txt -Raw | ii paste --token
ii drop .\incoming
ii pac --proxy socks5://192.168.1.10:1080
ii speed serve --port 9000
ii speed http://192.168.1.10:9000/ --duration 15s
```

`ii http [目录]` 是只读 nginx 风格目录服务：递归浏览、普通文件、Range、媒体播放和下载都与 `ii web` 目录页一致，但永远没有上传或写入接口。无目录时服务启动目录。

`ii paste [text]` 分享一段文本；没有位置参数时从 stdin 读取到 EOF。根页显示可复制文本，`raw` 返回 `text/plain`。`--ttl <duration>` 在正整数 `ms`、`s`、`m`、`h` 时间后自动关闭；文本以 `-` 开头时使用 `ii paste -- <text>`。

`ii drop [目录]` 只显示多文件上传页，支持现有的 1 MiB 分块续传，不列目录、不下载、不删除。无目录时首次上传会创建启动目录下的 `./ii/`；指定相对目录也以启动目录为基准，指定绝对目录原样使用。同名文件在完成后原子覆盖。

`ii pac --proxy <url>` 在根 URL 返回 `application/x-ns-proxy-autoconfig` PAC 内容。代理 URL 只能是无凭据、无 query/fragment 的 `http://host:port` 或 `socks5://host:port`；localhost、回环、RFC1918 IPv4、IPv6 loopback/link-local、`.local` 和无点主机名返回 `DIRECT`，其他目标使用指定代理。

`ii speed serve` 提供专用上下行测速端点；`ii speed <http-url>` 顺序跑下载和上传，默认各十秒，输出有效载荷、平均速率和总耗时。服务端和客户端使用 HTTP chunked 流，不伪造 `Content-Length`。

这五个服务都使用 `--port`、`--bind`、`--token [value]`：默认监听 IPv4 `0.0.0.0` 的随机端口，显式 IPv6 只监听 IPv6；终端打印二维码、主 LAN URL 与 `other:`。服务会被 `ii discover` 公告。

## `ii ftp`

```powershell
ii ftp
ii ftp .\shared --port 2121 --username alice --password secret
ii ftp .\shared --rate 8MiB --max 20 --upload false --delete false
ii ftp .\shared --passive-host 192.168.1.20 --passive-ports 49152-49200
ii ftp .\shared --tls
ii ftp .\shared --tls --cert .\fullchain.pem --key .\privkey.pem
ii ftp .\shared --tls --implicit
```

`ii ftp` 共享启动命令当前目录或一个已有目录。默认绑定 `0.0.0.0:21`、匿名登录、最多 100 个控制连接、不限速，上传、下载、删除、改名和新建目录全开。`--bind <ip>` 与 `--port <port>` 覆盖监听地址和端口；`0.0.0.0` 只是监听地址，终端会打印实际主 LAN IPv4 FTP URL 和 `other:` 网卡 URL。`--username` 和 `--password` 必须成对提供；提供后只接受该账号密码。

默认仅主动模式：不监听被动数据端口，`PASV` 和 `EPSV` 被拒绝。`--passive-ports [start-end]` 才启用被动模式，并保持主动模式可用；省略范围时使用 `49152-65535`。`--passive-host <IPv4|hostname>` 只能与 `--passive-ports` 同用，用于覆盖 PASV 响应地址。`--rate <bytes/s>` 复用发送端速率格式，限制所有上传和下载连接共享的总带宽；`--max <n>` 限制控制连接数，超限返回 FTP `421`。五个权限参数都只接受 `true` 或 `false`；`--delete false` 同时禁用 `DELE` 和 `RMD`，`--download false` 只禁用 `RETR`，目录浏览和元数据查询仍可用。

`--tls` 开启并强制 FTPS，控制和数据通道都必须使用 TLS。默认是显式 FTPS：客户端先收到 FTP 欢迎语，再发 `AUTH TLS`，默认端口仍为 `21`。未提供 `--cert` 和 `--key` 时，`ii` 只为当前进程生成自签证书，FTP 客户端需要手动接受；已有 PEM 完整证书链和私钥时必须成对提供。`--implicit` 只能和 `--tls` 同用，连接建立后立即进行 TLS 握手，未指定 `--port` 时默认使用 `990`。`--port 990` 只改变端口，不会自动切换到隐式 FTPS。`-k` 只用于自签 HTTPS relay，不适用于 `ii ftp`。

## `ii dav`

```powershell
ii dav
ii dav .\shared --port 8080 --bind 192.168.1.20 --token A1b2C3d4E5f6G7h8
ii dav .\shared --read-only
ii dav .\shared --port 8443 --username alice --password secret --tls --domain dav.example.com --cert .\fullchain.pem --key .\privkey.pem
```

`ii dav` 把当前目录或指定目录作为 WebDAV 根目录，默认可读写；支持 `OPTIONS`、`PROPFIND`、`GET`、`HEAD`、Range、`PUT`、`MKCOL`、`DELETE`、`MOVE`、`COPY`、`LOCK`、`UNLOCK`，上传支持 `Content-Length`、chunked body 和 `100-continue`。`--read-only` 禁止所有改写。`--token` 是 URL 路径令牌，不是账户认证。

`--username <username>` 与 `--password <password>` 必须成对提供，启用所有 DAV 方法的 HTTP Basic Auth。用户名不能为空，且不能包含 `:`、CR 或 LF；密码不能为空，且不能包含 CR 或 LF。`--password` 会进入 shell 历史和进程列表。

`--tls` 开启 HTTPS。没有 `--cert` 与 `--key` 时，`ii` 仅为当前进程生成自签证书；客户端必须手动信任。`--domain` 指定证书 DNS 名称和输出 URL；只允许与 `--tls` 同用。已有 PEM 完整证书链和私钥时，成对提供 `--cert`、`--key`；它们也要求 `--tls`。对公网提供服务时必须使用 TLS：可由 `ii dav --tls` 直接终止，或由 HTTPS 反向代理终止并把 `ii dav` 绑定到 `127.0.0.1`。明文 HTTP 下 Basic Auth 凭据可被窃听。

## `ii socks5`

```powershell
ii socks5
ii socks5 --port 1080
ii socks5 --bind 192.168.1.20 --username alice --password secret
```

`ii socks5` 是普通 SOCKS5 网络代理，不经过 Iroh、ticket 或 relay。默认监听 `0.0.0.0` 的随机端口，终端打印实际监听地址；`--port <port>` 固定端口，`--bind <ip>` 指定 IPv4 或 IPv6 监听地址。

支持 SOCKS5 `CONNECT`、`UDP ASSOCIATE`、`BIND`，以及 IPv4、IPv6 和域名目标。域名由代理端解析。没有认证参数时使用 SOCKS5 无认证方式；`--username <user>` 和 `--password <pass>` 必须成对提供，提供后只接受 RFC 1929 用户名密码认证。

## 代理、转发与网络工具

```powershell
ii proxy --port 8080
ii proxy --username alice --password secret
ii tcp db.internal:5432 --port 15432
ii udp game.internal:27015 --port 27015
ii ping api.example.com:443 --count 4
ii port api.example.com 80 443 8443
ii health https://api.example.com/health --interval 10s
ii wake aa:bb:cc:dd:ee:ff --broadcast 192.168.1.255
```

`ii proxy` 是普通 HTTP 正向代理。支持 HTTP/1.0、HTTP/1.1 absolute-form 请求和 `CONNECT`；普通请求强制 `Connection: close` 后转发一条请求，`CONNECT` 进入双向 TCP 流。它不做 TLS 解密、缓存或 WebSocket 代理。`--username` 和 `--password` 必须成对提供；提供后只接受 `Proxy-Authorization: Basic`。

`ii tcp <host:port>` 为每个进入的 TCP 连接双向转发到固定目标。`ii udp <host:port>` 为每个客户端地址维护一个上游 UDP socket，域名在创建会话时解析，五分钟无流量后清理。两者都支持 IPv4、IPv6 或域名目标，`--port` 与 `--bind` 控制本地 listener。

`ii ping <host:port>` 是 TCP connect 延迟探测，不发送原始 ICMP；默认四次、间隔一秒、每次三秒超时，并输出 min/avg/max。`ii port <host> <port...>` 并发检查每个 TCP 端口，按输入顺序输出 `open`、`closed` 或 `timeout`。`ii health <http-url|host:port>` 对 HTTP/HTTPS 的 `2xx/3xx` 判健康，对裸地址检查 TCP；不带 `--interval` 只检查一次并以失败状态退出，带间隔时只在状态变化时输出并持续运行。`ii wake <mac>` 发送 102 字节 Wake-on-LAN magic packet，MAC 可用冒号或短横线，默认发往 `255.255.255.255:9`。

## `ii discover`

```powershell
ii discover
ii discover --json
```

在本地网络监听三秒，发现正在运行的 `ii send -t`、`ii web`、`ii dav`、`ii http`、`ii paste`、`ii drop`、`ii pac` 和 `ii speed serve`，并输出可直接使用的 `ii recv <ticket>` 或服务 URL。发现同时使用 IPv4 广播和 IPv6 链路本地 all-nodes 多播；公告会向同一 LAN 暴露 ticket 或 token URL，不是访问控制机制。`--json` 输出 JSON Lines，stdout 不混入其他文本。

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

ticket 内有一次随机访问密钥。持有 ticket 的设备可以接入，直到 A 按 `Ctrl+C` 停止 tunnel；不要泄露 ticket。tunnel 本身不支持 UDP、SOCKS 或反向 tunnel；普通 SOCKS5 代理使用独立的 `ii socks5`。

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
: 输出接收过程的分段耗时、地址统计、最终选中的 LAN/直连/relay 路径、RTT、写入字节数和平均速度，便于排查为什么慢。

`--checksum md5|sha256`
: 接收完成后计算并输出实际文件或目录 tar 流；不与发送端自动比对，且不能与 `--stdout` 同用。

`--quic-port <1..65535>`
: 固定 P2P Iroh UDP 端口；后端 ticket 不能使用该参数。

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

- 指定一个 relay 时强制使用它
- 重复指定多个 relay 时只在这些显式 relay 中探测并选择最快可达节点
- 不影响默认 n0 relay 的自动路径选择

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

`doctor` 默认只读取本地配置和运行环境，不创建网络服务、不修改配置，也不改变端口或 relay 设置。

```powershell
ii doctor --nat
```

`--nat` 会创建一个短生命周期的默认 Iroh endpoint，报告：

- 实际绑定的 UDP socket 及 IPv4/IPv6 可用性
- Iroh net-report 的 UDP 探测结果和 NAT 映射是否随目标变化
- 首选 relay 与 relay 在线结果
- `hairpin` 明确显示为 `unavailable`，因为当前 Iroh 公开 API 没有 hairpin 探测字段

探测结束后 endpoint 立即关闭；该命令不保存网络状态，也不伪造缺失的 hairpin 结果。

默认 `doctor` 会输出：

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
- S3、R2 中转：内部精简 SigV4/HTTP 客户端
- Azure Blob 中转：内部精简 Block Blob REST 客户端
- WebDAV 中转：内部精简 HTTP/WebDAV 客户端
- FTP 中转：`suppaftp`
- SFTP 中转：`russh`、`russh-sftp`

## 源码对照

- [iroh-relay/src/main.rs](https://github.com/n0-computer/iroh/blob/main/iroh-relay/src/main.rs)
- [iroh-relay/src/server.rs](https://github.com/n0-computer/iroh/blob/main/iroh-relay/src/server.rs)
- [Iroh Docs: Add a relay](https://docs.iroh.computer/add-a-relay)
