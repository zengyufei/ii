# ii 用户手册

`ii` 是唯一对外品牌和唯一用户入口。用户只需要记 `ii`，不用记 `sendme`、`provide/get`、`iroh-relay`，也不用接触 `hash`、`peer id`、`token` 这些底层词。

## 一句话

`ii send` 发，`ii recv` 收，`ii relay` 管中继，`ii doctor` 查问题，`ii version` 看版本。

## 命令总览

```text
ii send [<path>] [--name <name>] [-t] [-c] [-o <path>] [--s3] [--webdav] [--profile <name>] [-d] [-p] [--local] [--relay <url>] [--no-relay]
ii recv <ticket> [-o <dir>] [--stdout] [--overwrite] [--resume] [--local] [--trace]
ii relay [--config <path>] [--port <port>] [--metrics <port>] [--tls <domain> --cert <path> --key <path>]
ii doctor
ii version
```

## 核心规则

- 命令要直：`send` 就是发送，`recv` 就是接收。
- 用户只复制 `ticket`，不手工拼内部地址。
- 默认先走直连和局域网，必要时再走公网 relay。
- 需要显式限制路径时，用 `--local`、`--relay`、`--no-relay`。
- `ii relay` 是运维命令，不是用户日常发文件要记的东西。
- `--s3` 和 `--webdav` 是可选中转后端，第一次会初始化本机 `ii.toml`。

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

`--local`
: 只走局域网优先路径，不走公网发现，不走公网 relay。

`--relay <url>`
: 强制指定 relay。  
  如果这里填的是域名，那这个域名必须先解析到 relay 服务器的公网地址，通常由 relay 运维方配置 `A/AAAA` 或 `CNAME`。  
  客户端只负责使用这个 URL，不负责替你配 DNS。

`--no-relay`
: 禁用 relay，只允许直连和局域网路径。

`--s3`
: 走对象存储后端，不走 peer/relay 路径。默认 profile 是 `default`，默认 provider 是 Cloudflare R2。  
  如果本机还没有配置，`ii` 会在终端里依次提示 `Account ID`、`Bucket`、`Access Key ID`、`Secret Access Key`，成功后把配置写到平台默认路径：Windows 是 `ii.exe` 同目录下的 `ii.toml`，Linux/macOS/其他 Unix-like 是 `/etc/ii/ii.toml`。  
  之后再执行 `ii send ... --s3` 时，会直接复用这份配置。

`--profile <name>`
: 只在 `--s3` 或 `--webdav` 模式下生效，用来选择 `ii.toml` 里的后端 profile。  
  例子：`ii send .\file.zip --s3 --profile work`、`ii send .\file.zip --webdav --profile nas`。  
  S3 和 WebDAV 不指定时都默认使用 `default`。旧的 `[storage.s3.cloudflare]` 会自动兼容迁移为 S3 的 `default` profile。

`-d`
: 只在 `--s3` 或 `--webdav` 模式下生效。接收端拿到文件后，会尝试删除中转端里的对应对象；删除失败会忽略，不影响下载结果。

`--webdav`
: 走 WebDAV 中转后端，不走 peer/relay 路径。  
  如果本机还没有配置，`ii` 会在终端里依次提示 `URL`、`Username`、`Password`，三项都是明文输入。上传成功后把配置写到平台默认路径：Windows 是 `ii.exe` 同目录下的 `ii.toml`，Linux/macOS/其他 Unix-like 是 `/etc/ii/ii.toml`。  
  文件和 stdin 会按 `remote_dir/<md5>` 存到 WebDAV；同 MD5 对象已存在时不重复上传。

`-p`
: 只在 `--webdav` 模式下生效。生成便携 WebDAV ticket，把 WebDAV URL、用户名和密码直接写进 ticket。  
  这能保证对方没有 WebDAV 配置也能 `ii recv`，但谁拿到 ticket 谁就拿到了这次 WebDAV 访问凭据。接收端成功接收后，会把 ticket 内的 WebDAV 配置写入本机 `ii.toml`，后续不需要再配置。

### 路径规则

- `--s3`、`--webdav`、`--local`、`--relay`、`--no-relay` 互斥。
- 默认不需要用户选 relay。
- 如果没有局域网或直连可用，默认会自动退到公网 relay。

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
- 排查慢的时候，先跑一次默认模式，再跑一次 `--local` 对比；如果 `--local` 明显快，问题通常在公网发现或 relay 路径，不在本地写盘。
- WebDAV 普通 ticket 不带凭据，接收端首次使用时会提示输入 `URL`、`Username`、`Password`，下载成功后保存到 `ii.toml`；WebDAV `-p` ticket 会直接使用 ticket 内的 URL、用户名和密码，并在成功接收后保存到本机 `ii.toml`。

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

例外：`ii send --webdav -p` 会把 WebDAV URL、用户名和密码放进 ticket，这是为了让没有本机配置的接收方也能直接 `ii recv`。

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

`ii relay` 是 relay 服务入口，对外只保留这一层。

不用：

```powershell
ii relay serve
iroh-relay
```

### 用法

```powershell
ii relay
ii relay --config .\relay.toml
ii relay -H 8080
ii relay --tls relay.example.com -H 8443 --cert .\fullchain.pem --key .\privkey.pem
```

### 默认启动

直接运行：

```powershell
ii relay
```

第一次会生成 `relay.toml`，然后启动一个只用 HTTP 的 relay：

- 监听 `0.0.0.0:3340`，服务器只需开放 `3340/tcp`
- 直接使用公网 IP，不需要域名、DNS、HTTPS 或 TLS 证书
- 不启动 HTTPS、QUIC 地址发现和 metrics
- 客户端指定 `http://服务器公网IP:3340`

```powershell
ii send .\video.mp4 --relay http://203.0.113.10:3340
```

这是默认路径。公网 HTTP 没有 TLS 保护，只适合临时、受信任的网络；要长期公开提供 relay，再配置 HTTPS。

旧版自动生成的不完整 TLS 模板会被识别并忽略，`ii relay` 会回退到 HTTP，不再因缺域名启动失败。

### 配置文件路径

- 不传 `--config` 时，按平台找默认配置：
  - Windows: `ii.exe` 同目录下的 `relay.toml`
  - Linux/macOS/其他 Unix-like: `/etc/ii/relay.toml`
- 如果默认文件不存在，`ii relay` 会先生成它，再继续启动
- 传了 `--config` 时，就用你指定的路径
- 命令行端口参数优先级高于 `relay.toml`
- 如果配置文件已存在，端口参数只影响本次启动，不自动改写文件

### HTTP 端口

默认端口 `3340` 不占 `80/443`，不需要 root 权限，也不会和常见的 Nginx 配置冲突。需要改 HTTP 端口时：

```powershell
ii relay -H 8080
```

客户端地址也要带上端口：

```powershell
ii send .\video.mp4 --relay http://203.0.113.10:8080
```

端口参数：

- 默认或未传 `--tls` 时，`--port <port>` / `-H <port>` 是 HTTP relay 端口，对应 `http_bind_addr`
- 传了 `--tls` 时，`-H <port>` 改为 HTTPS relay 端口，默认 `443`
- `--metrics <port>` / `-M <port>`：开启 metrics 并设置 metrics 端口，对应 `metrics_bind_addr`

### HTTPS 和域名

HTTPS 是显式进阶模式，必须同时给域名、完整证书链和私钥：

```powershell
ii relay --tls relay.example.com -H 8443 --cert .\fullchain.pem --key .\privkey.pem
```

域名必须已经解析到 relay 服务器；`--cert` 指向 PEM 格式的完整证书链，`--key` 指向匹配的 PEM 私钥。证书的 SAN 必须包含 `--tls` 指定的域名。`ii` 不申请、不续期证书，也不要求联系邮箱。

客户端使用 HTTPS：

```powershell
ii send .\video.mp4 --relay https://relay.example.com:8443
```

TLS 模式不会在公网开放 HTTP relay。底层所需的 HTTP 探测监听只绑定本机随机端口，因此下面的地址不成立：

```powershell
ii send .\video.mp4 --relay http://relay.example.com:8443
```

`--config` 也可保存手工 TLS 配置：

```toml
[tls]
domain = "relay.example.com"
https_bind_addr = "0.0.0.0:8443"
cert_path = "/etc/ii/fullchain.pem"
key_path = "/etc/ii/privkey.pem"
```

端口职责：

- `3340/tcp`：默认 HTTP relay
- `443/tcp` 或 `-H` 指定的 TCP 端口：启用 `--tls` 后的 HTTPS relay
- `9090/tcp`：metrics，默认关闭；官方 release 当前未启用 metrics，传 `-M` 会报错

- 反向代理必须支持 WebSocket / HTTP upgrade。
- `--cert` 或 `--key` 缺任意一个都会拒绝启动。

### TLS 来源

TLS 只读取用户显式指定的 `--cert` 和 `--key`，或 `relay.toml` 里的 `cert_path` 和 `key_path`。ACME、LetsEncrypt 和证书自动续期不属于 `ii`。

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

## 源码对照

- [iroh-relay/src/main.rs](https://github.com/n0-computer/iroh/blob/main/iroh-relay/src/main.rs)
- [iroh-relay/src/server.rs](https://github.com/n0-computer/iroh/blob/main/iroh-relay/src/server.rs)
- [Iroh Docs: Add a relay](https://docs.iroh.computer/add-a-relay)
