# ii WebDAV 中转方案

## 目标

给 `ii` 增加一个可选的 WebDAV 中转模式，让用户在自建 NAS、Nextcloud、Alist、公司内网文件服务等环境里，先把文件上传到 WebDAV，再由 `ii recv <ticket>` 拉取。

WebDAV 不是主默认后端。默认推荐仍然是 Cloudflare R2 这类 S3-compatible 中转；WebDAV 作为第二后端，面向自建环境和已有文件服务。

## 核心原则

- 命令行优先，不做网页表单。
- 首次配置时，允许在 `ii send` 的终端里逐项输入。
- 配置成功后写入 `ii.toml`，下次直接复用。
- `ii recv` 不接触 WebDAV 密钥，只消费 ticket。
- ticket 里只放取回文件所需的信息，不放长期凭据。

## 用户流程

第一次运行：

```powershell
ii send .\video.mp4 --webdav
```

如果还没有 WebDAV 配置，`ii` 暂停当前发送流程，在同一个命令行窗口里提示用户完成初始化：

```text
ii: WebDAV is not configured.

WebDAV URL:
Username:
Password:
Remote folder:
Public download base URL (optional):
```

输入完成后，`ii` 继续执行：

- 校验 WebDAV 连接
- 写入 `ii.toml`
- 上传文件到 WebDAV
- 生成 ticket

第二次及以后运行：

```powershell
ii send .\video.mp4 --webdav
```

如果配置已存在，就直接上传，不再询问。

## 配置文件

建议使用 `ii.toml` 保存 WebDAV 配置。默认配置块只保留一个 profile：

```toml
[storage]
backend = "webdav"
profile = "default"

[storage.webdav.default]
url = "https://dav.example.com/remote.php/dav/files/user/"
username = "user"
password = "app-password"
remote_dir = "ii/"
public_base_url = ""
```

说明：

- `url` 是 WebDAV 根地址。
- `username` 和 `password` 是访问凭据，建议使用 app password，不要用主账号密码。
- `remote_dir` 是对象落点目录。
- `public_base_url` 可选。填了以后，ticket 可直接给出普通 HTTPS 下载地址。

## ticket 设计

`ii send --webdav` 成功后生成新的 ticket，ticket 只承担“让接收端找到文件”的职责。

推荐放入 ticket 的内容：

- 文件名
- 文件大小
- 内容 hash
- WebDAV 对象路径
- 如果有 `public_base_url`，则放公开下载 URL

不建议放入 ticket 的内容：

- WebDAV username
- WebDAV password
- 长期可用的登录凭据

## 交互细节

命令行交互要简单，顺序固定，用户只需要一路回车或填值。

建议顺序：

1. `WebDAV URL`
2. `Username`
3. `Password`
4. `Remote folder`
5. `Public download base URL`

补充规则：

- `Password` 输入时不回显。
- 每输入一项，立刻进入下一项。
- 如果某项已经在配置里存在，就直接跳过。
- 如果校验失败，回到当前项重输，不要整段重来。

## 两种使用模式

### 公开下载模式

如果用户配置了 `public_base_url`，`ii` 在上传后可以把 ticket 里的下载地址指向一个普通 HTTPS URL。

这时接收端只需要：

```powershell
ii recv <ticket>
```

不需要 WebDAV 账号密码。

### 私有 WebDAV 模式

如果没有 `public_base_url`，ticket 只保存 WebDAV 对象路径。

这时接收端要能访问同一份 WebDAV 配置，或者至少能访问同一 WebDAV 服务。

## 失败回退

如果自动初始化失败，`ii` 仍然要让用户能继续完成配置。

可接受的回退方式：

- 重新提示当前字段
- 显示缺失项
- 允许用户中断后再次执行 `ii send --webdav` 继续补齐

不建议的方式：

- 直接退出并让用户手工猜配置字段
- 要求用户先编辑复杂 JSON
- 要求用户先做网页登录再回到命令行

## 适用场景

WebDAV 更适合：

- 自建 NAS
- Nextcloud
- Alist
- 公司内网文件服务
- 已经有 WebDAV 账号的环境

WebDAV 不太适合：

- 想要“发给别人就能直接收”的零配置外发
- 没有公开下载入口的私有文件服务

## 设计取向

这个方案的目标不是把 WebDAV 做成一个独立云存储工具，而是把它做成 `ii` 的一种传输后端。

所以体验上要遵守三条：

- 第一次麻烦一次，之后不再麻烦。
- 配置尽量少，字段尽量直白。
- 默认行为简单，手动改动留给高级用户。
