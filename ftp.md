# ii FTP 中转说明

`ii send --ftp` 是 `ii` 的 FTP 中转模式。发送端先把文件、stdin 内容或目录归档上传到 FTP，再生成 `ii recv <ticket>`；接收端按 ticket 从 FTP 拉取。

FTP 只支持明文 `ftp://`。用户名、密码、文件内容和控制命令都可能被网络上的人读取。不要通过公网或不可信网络使用它；需要加密传输时用 `--sftp`。

## 命令

```powershell
ii send .\video.mp4 --ftp
ii send .\video.mp4 --ftp --profile legacy
ii send .\video.mp4 --ftp -p
ii send .\video.mp4 --ftp -d
ii send .\video.mp4 --ftp -p -d
ii recv <ticket>
```

参数含义：

- `--ftp`: 使用 FTP 中转，而不是点对点传输。
- `--profile <name>`: 指定 FTP profile，只支持长参数，没有 `-P`。
- `-p`: 生成便携 ticket，把 FTP URL、用户名、密码和远端目录写入 ticket。
- `-d`: 接收端成功接收后尝试删除远端对象。

`--ftp` 和 `--s3`、`--webdav`、`--sftp`、`--local`、`--relay`、`--no-relay` 互斥。`-p` 只能和 `--webdav`、`--ftp` 或 `--sftp` 一起使用。`--profile` 与 `-d` 只能在中转后端模式下使用。

## 配置路径

FTP 配置写入 `ii.toml`，默认路径固定：

- Windows: `ii.exe` 同目录下的 `ii.toml`
- Linux/macOS/其他 Unix-like: `/etc/ii/ii.toml`

第一次执行 `ii send <file> --ftp` 或接收普通 FTP ticket 时，如果缺配置，`ii` 会在交互终端提示 `FTP URL`、`Username`、`Password`。配置只在上传或下载成功后写入。

## profile 规则

不传 `--profile` 时，FTP 使用默认 profile `default`。传 `--profile legacy` 时，会读取或创建：

```toml
[storage.ftp.legacy]
```

普通 FTP ticket 只记录 profile 名和对象 key。接收端没有对应 profile 时，会在 `ii recv <ticket>` 时提示输入配置。

## 配置格式

```toml
[storage.ftp.default]
url = "ftp://ftp.example.com:21"
username = "ii"
password = "password"
remote_dir = "ii/"
```

字段说明：

- `url`: 必须是 `ftp://主机[:端口]`。不支持 FTPS、HTTPS 或 `sftp://`。
- `username` / `password`: FTP 凭据，保存在本机 `ii.toml`，也是明文。
- `remote_dir`: 远端对象目录，默认 `ii/`。

## 对象命名和去重

文件和 stdin 这类有内容 MD5 的输入，远端对象 key 使用：

```text
<remote_dir>/<md5>
```

默认就是 `ii/<md5>`。同 MD5 对象已经存在时，`ii send --ftp` 跳过上传，直接复用已有对象并生成新的 ticket。目录会先打成 tar 归档再上传，使用随机对象 key，不做跨次 MD5 去重。

## 普通 ticket

默认命令：

```powershell
ii send .\video.mp4 --ftp
```

普通 FTP ticket 携带文件名、文件大小、内容 MD5、payload 类型、FTP object key、profile 名和可选的接收后删除标记。它不携带 FTP URL、用户名或密码。

## 便携 ticket

```powershell
ii send .\video.mp4 --ftp -p
```

`-p` 会把 FTP URL、用户名、密码和远端目录直接写进 ticket。ticket 只有编码，没有加密；拿到 ticket 的人可以用这些凭据访问 FTP 服务。接收成功后，接收端会把这些配置保存到本机 `ii.toml`。

## 接收、删除和失败处理

文件接收沿用默认规则：目标不存在就下载，目标更短就尝试续传，同名同尺寸且 MD5 一致就跳过，同名不同内容就覆盖。目录下载后解包到目标目录；`--stdout` 不支持目录 ticket。

加 `-d` 时，`ii recv` 在成功下载或跳过相同文件后尝试删除远端对象。删除失败只记录或忽略，不影响本地接收结果。

非交互终端缺配置时会直接报错，提示先在交互终端初始化一次或手工编辑 `ii.toml`。上传和下载都会显示实时进度、速率和最终耗时。
