# ii WebDAV 中转说明

`ii send --webdav` 是 `ii` 的 WebDAV 中转模式。发送端先把文件、stdin 内容或目录归档上传到 WebDAV，再生成 `ii recv <ticket>`；接收端按 ticket 从 WebDAV 拉取。

WebDAV 适合 NAS、Nextcloud、Alist、公司内网文件服务等已有 WebDAV 环境。默认推荐的公网中转仍然是 Cloudflare R2/S3；WebDAV 是第二后端。

## 命令

```powershell
ii send .\video.mp4 --webdav
ii send .\video.mp4 --webdav --profile nas
ii send .\video.mp4 --webdav -p
ii send .\video.mp4 --webdav -d
ii send .\video.mp4 --webdav -p -d
ii recv <ticket>
```

参数含义：

- `--webdav`: 使用 WebDAV 中转，而不是点对点传输。
- `--profile <name>`: 指定 WebDAV profile，只支持长参数，没有 `-P`。
- `-p`: 生成便携 ticket，把 WebDAV URL、用户名、密码和鉴权方式写入 ticket。
- `-d`: 接收端成功接收后尝试删除远端对象。

`--webdav` 和 `--s3`、`--local`、`--relay`、`--no-relay` 互斥。`-p` 只能和 `--webdav` 一起使用。`--profile` 只能和 `--s3` 或 `--webdav` 一起使用。`-d` 只能和 `--s3` 或 `--webdav` 一起使用。

## 配置路径

WebDAV 配置写入 `ii.toml`，默认路径固定：

- Windows: `ii.exe` 同目录下的 `ii.toml`
- Linux/macOS/其他 Unix-like: `/etc/ii/ii.toml`

`ii.exe` 只运行普通命令不会主动创建配置文件。第一次执行需要配置的命令，例如 `ii send <file> --webdav` 或接收普通 WebDAV ticket，才会在配置缺失并且传输成功后写入 `ii.toml`。

## profile 规则

不传 `--profile` 时，WebDAV 使用默认 profile `default`。

传 `--profile nas` 时，会读取或创建：

```toml
[storage.webdav.nas]
```

普通 WebDAV ticket 会记录 profile 名。接收端如果没有对应 profile，会在 `ii recv <ticket>` 时提示输入 WebDAV 配置。

首次缺配置时，`ii` 会在当前终端提示：

```text
ii: WebDAV is not configured.

URL:
Username:
Password:
```

`Password` 是明文输入，不隐藏。发送端配置只有在上传成功后才保存；接收端配置只有在下载成功后才保存。

## 配置格式

```toml
[storage]
backend = "webdav"
profile = "default"

[storage.webdav.default]
url = "https://dav.example.com/remote.php/dav/files/user/"
username = "user"
password = "app-password"
remote_dir = "ii/"
auth = "basic"
```

字段说明：

- `url`: WebDAV 根地址。
- `username` / `password`: WebDAV 凭据，建议用 app password。
- `remote_dir`: 远端对象目录，默认 `ii/`。
- `auth`: 默认 `basic`，需要 Digest 时手工改成 `digest`。

多个 profile 可以并存：

```toml
[storage.webdav.nas]
url = "https://nas.example.com/dav/"
username = "user"
password = "password"
remote_dir = "ii/"
auth = "basic"
```

使用时：

```powershell
ii send .\video.mp4 --webdav --profile nas
```

## 对象命名和去重

文件和 stdin 这类有内容 MD5 的输入，远端对象 key 使用：

```text
<remote_dir>/<md5>
```

默认就是：

```text
ii/<md5>
```

中间没有 `md5/` 目录，也不保留文件后缀。真实文件名保存在 ticket 里，接收端落盘时仍按 ticket 文件名保存。

如果同 MD5 对象已经存在，`ii send --webdav` 会跳过上传，直接复用已有对象并生成新的 ticket。

目录会先打成 tar 归档再上传；目录归档走随机对象 key，不做跨次 MD5 去重。

## 普通 ticket

默认命令：

```powershell
ii send .\video.mp4 --webdav
```

生成普通 WebDAV ticket。它携带：

- 文件名
- 文件大小
- 内容 MD5
- payload 类型：文件、stdin 或目录
- WebDAV object key
- profile 名
- 如果发送端加了 `-d`，还会记录接收后删除远端对象

普通 ticket 不携带：

- WebDAV URL
- WebDAV username
- WebDAV password

因此接收端执行 `ii recv <ticket>` 时，必须能从本机 `ii.toml` 找到对应 profile；如果找不到，`ii recv` 会在当前终端提示输入 `URL`、`Username`、`Password`，下载成功后保存到本机 `ii.toml`。

## 便携 ticket

发送端执行：

```powershell
ii send .\video.mp4 --webdav -p
```

`-p` 会把 WebDAV URL、用户名、密码和鉴权方式写进 ticket。谁拿到 ticket，谁就能用这些信息读取对应对象。

接收端没有 WebDAV 配置也可以直接执行：

```powershell
ii recv <ticket>
```

接收成功后，接收端会把 ticket 内的 WebDAV URL、用户名、密码和鉴权方式写入本机 `ii.toml`，profile 名沿用 ticket 里的 profile 名。当前便携 ticket 不携带 `remote_dir` 字段，写入本机配置时 `remote_dir` 使用默认 `ii/`。

`-p` 是方便但不安全的模式，只适合你愿意把这组 WebDAV 凭据交给接收方的场景。

## 接收和删除

接收端继续复用本地文件规则：目标不存在就下载，目标更短就续传，同名同尺寸且 MD5 相同就跳过，同名不同内容就覆盖。

如果发送端加了 `-d`：

```powershell
ii send .\video.mp4 --webdav -d
ii send .\video.mp4 --webdav -p -d
```

`ii recv` 在成功下载后会尝试删除远端对象；如果本地文件已经完整且 MD5 相同而直接跳过，也会尝试删除远端对象。删除失败只记录或忽略，不影响本地接收结果。

## 进度

`ii send --webdav` 上传时会显示实时进度、速率和最终耗时。`ii recv` 从 WebDAV 下载时也会显示实时进度、速率和最终耗时。

## 失败处理

非交互终端里缺配置时，`ii send --webdav` 或接收普通 WebDAV ticket 会直接报错，提示先在交互终端初始化一次或手工编辑 `ii.toml`。

交互终端里缺字段时，`ii` 会按顺序提示输入。当前实现不会在单次运行中循环重输错误字段；如果上传、下载或认证失败，本次配置不会保存，用户需要重新执行命令或手工修正 `ii.toml`。
