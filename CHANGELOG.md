# 变更记录

本文件记录 `ii` 的重要变更。默认中文版本在这里，英文版本见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## Unreleased

暂无。

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
