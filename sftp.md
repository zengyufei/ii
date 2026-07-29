# ii SFTP 中转说明

`ii send --sftp` 是 `ii` 的 SFTP 中转模式。发送端先把文件、stdin 内容或目录归档上传到 SFTP，再生成 `ii recv <ticket>`；接收端按 ticket 从 SFTP 拉取。

SFTP 通过 SSH 加密传输，支持密码和私钥认证。但当前实现不保存或校验 known-hosts：每次连接都会打印服务器 SSH SHA-256 主机指纹并直接接受。攻击者能替换服务器并实施中间人攻击。不要把这当作已验证的主机身份。

## 命令

```powershell
ii send .\video.mp4 --sftp
ii send .\video.mp4 --sftp --profile server
ii send .\video.mp4 --sftp -p
ii send .\video.mp4 --sftp -d
ii send .\video.mp4 --sftp -p -d
ii recv <ticket>
```

参数含义：

- `--sftp`: 使用 SFTP 中转，而不是点对点传输。
- `--profile <name>`: 指定 SFTP profile，只支持长参数，没有 `-P`。
- `-p`: 生成便携 ticket。密码认证写入密码；私钥认证写入私钥文本和可选口令。
- `-d`: 接收端成功接收后尝试删除远端对象。

`--sftp` 和 `--s3`、`--webdav`、`--ftp`、`--local`、`--relay`、`--no-relay` 互斥。`-p` 只能和 `--webdav`、`--ftp` 或 `--sftp` 一起使用。`--profile` 与 `-d` 只能在中转后端模式下使用。

## 配置路径

SFTP 配置写入 `ii.toml`，默认路径固定：

- Windows: `ii.exe` 同目录下的 `ii.toml`
- Linux/macOS/其他 Unix-like: `/etc/ii/ii.toml`

第一次执行 `ii send <file> --sftp` 或接收普通 SFTP ticket 时，如果缺配置，`ii` 会在交互终端提示主机、用户名和认证方式。配置只在上传或下载成功后写入。

## profile 规则

不传 `--profile` 时，SFTP 使用默认 profile `default`。传 `--profile server` 时，会读取或创建：

```toml
[storage.sftp.server]
```

普通 SFTP ticket 只记录 profile 名和对象 key。接收端没有对应 profile 时，会在 `ii recv <ticket>` 时提示输入配置。

## 配置格式

密码认证：

```toml
[storage.sftp.default]
host = "sftp.example.com"
port = 22
username = "ii"
remote_dir = "ii/"
auth = "password"
password = "password"
```

私钥认证：

```toml
[storage.sftp.server]
host = "sftp.example.com"
port = 22
username = "ii"
remote_dir = "ii/"
auth = "private-key"
private_key_path = "C:\\Users\\you\\.ssh\\id_ed25519"
private_key_passphrase = "optional-passphrase"
```

字段说明：

- `host`: 只填主机名或 IP，不填 `sftp://`、端口或路径。
- `port`: SSH 端口，默认 `22`。
- `username`: SSH 用户名。
- `remote_dir`: 远端对象目录，默认 `ii/`。
- `auth`: `password` 或 `private-key`。
- `password`: 密码认证需要的明文密码。
- `private_key_path`: 私钥认证需要的本地私钥文件路径。
- `private_key_passphrase`: 可选私钥口令。

## 对象命名和去重

文件和 stdin 这类有内容 MD5 的输入，远端对象 key 使用 `<remote_dir>/<md5>`，默认就是 `ii/<md5>`。同 MD5 对象已经存在时，`ii send --sftp` 跳过上传，直接复用已有对象并生成新的 ticket。目录会先打成 tar 归档再上传，使用随机对象 key，不做跨次 MD5 去重。

## 普通 ticket

普通 SFTP ticket 携带文件名、文件大小、内容 MD5、payload 类型、SFTP object key、profile 名和可选的接收后删除标记。它不携带主机、用户名、密码、私钥或私钥口令。

## 便携 ticket

```powershell
ii send .\video.mp4 --sftp -p
```

密码认证的 `-p` ticket 会写入主机、端口、用户名、远端目录和密码。私钥认证的 ticket 会额外写入完整私钥文本和可选口令。ticket 只有编码，没有加密；拿到 ticket 的人可以取得这些凭据。

接收成功后，密码配置写入本机 `ii.toml`。便携私钥会保存为配置目录下 `ii-keys/<profile>.key`，Unix 上权限设为 `0600`；`ii.toml` 只保存该文件路径。Windows 不会额外设置 ACL。

## 接收、删除和失败处理

文件接收沿用默认规则：目标不存在就下载，目标更短就尝试续传，同名同尺寸且 MD5 一致就跳过，同名不同内容就覆盖。目录下载后解包到目标目录；`--stdout` 不支持目录 ticket。

加 `-d` 时，`ii recv` 在成功下载或跳过相同文件后尝试删除远端对象。删除失败只记录或忽略，不影响本地接收结果。

非交互终端缺配置时会直接报错，提示先在交互终端初始化一次或手工编辑 `ii.toml`。上传和下载都会显示实时进度、速率和最终耗时。
