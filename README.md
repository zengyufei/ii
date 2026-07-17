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

`ii` 面向临时传文件/夹：

直发目录 / 一次即走 / 持续发送 / 自动复制到粘贴板或落盘

自动找路 / 局域网优先 / 可公网中继或 `--s3` / `--webdav` 中继

断点续收 / 秒传跳过 / 冲突覆盖 / 支持传完清理中转

进度速率 / 完成耗时 / 支持诊断本机 / 支持自建中继

## 快速开始

在发送端执行：

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

在接收端执行：

```powershell
ii recv ii1k7v...x9a
```

## 常见场景

同事之间临时传一个文件：

```powershell
ii send .\report.pdf
ii recv ii1k7v...x9a
```

发送端和接收端的实际样子：

![发送端截图](screenshot/发送.png)

![接收端截图](screenshot/接收.png)

指定保存目录：

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

断网或传到一半失败后，重新执行同一条 `ii recv` 就会继续接收；如果目标文件已经完整且内容相同，会直接跳过；如果同名但内容不同，会覆盖。

`ii send` 和 `ii recv` 都会在终端里实时显示传输进度和速率；完成后会打印最终耗时。`--trace` 主要用于诊断，方便排查连接慢在哪里。

## 发送目录

目录可以直接发送：

```powershell
ii send .\my-folder
```

接收端：

```powershell
ii recv ii1k7v...x9a -o D:\Downloads
```

接收结果是 `D:\Downloads\my-folder`，不会变成 `my-folder\my-folder` 两层。

## 进阶用法

默认发送端只服务一次接收。需要保持发送端不退出时，用 `-t`：

```powershell
ii send .\my-folder -t
```

复制接收命令到剪贴板：

```powershell
ii send .\video.mp4 -c
```

把接收命令写到文件：

```powershell
ii send .\video.mp4 -o recv.txt
```

从 stdin 发送：

```powershell
tar czf - .\project | ii send --name project.tar.gz
```

接收到 stdout：

```powershell
ii recv ii1k7v...x9a --stdout > project.tar.gz
```

局域网优先，不走公网中继：

```powershell
ii send .\file.zip --local
ii recv ii1k7v...x9a --local
```

通过 S3/R2 中转：

```powershell
ii send .\video.mp4 --s3
ii recv ii1k7v...x9a
```

首次使用会在命令行里提示填写 Cloudflare R2 配置，成功后写入本机 `ii.toml`，以后直接复用。

通过 WebDAV 中转：

```powershell
ii send .\video.mp4 --webdav
ii recv ii1k7v...x9a
```

选择指定后端 profile：

```powershell
ii send .\video.mp4 --s3 --profile work
ii send .\video.mp4 --webdav --profile nas
```

如果接收方没有 WebDAV 配置，可以用便携 ticket：

```powershell
ii send .\video.mp4 --webdav -p
```

`-p` 会把 WebDAV URL、用户名和密码写进 ticket，方便但不安全，只适合你信任 ticket 接收方的场景。

接收成功后，`-p` ticket 内的 WebDAV 配置会写入接收端本机 `ii.toml`。如果希望接收后清理 WebDAV 上的对象，可以加 `-d`：

```powershell
ii send .\video.mp4 --webdav -p -d
```

## 诊断

排查为什么慢：

```powershell
ii recv ii1k7v...x9a --trace
ii recv ii1k7v...x9a --local --trace
```

检查本机网络、端口、权限和版本信息：

```powershell
ii doctor
ii version
```

## 自托管 Relay

普通发文件不需要先理解 relay。只有你要自建中继服务，或者公司网络环境需要固定中继入口时，才需要看这一段。

启动自签 HTTPS relay：

```powershell
ii relay --public https://服务器公网IP:8443
```

也可以使用域名：

```powershell
ii relay --public https://relay.example.com
```

`--public` 是客户端实际访问的公网 HTTPS 地址，必须是 `https://主机[:端口]`。首次启动会自动在 relay 状态目录生成并持久化自签证书和私钥。默认监听 `--public` 的端口，未写端口就是 `443`；NAT 或反向代理需要转到不同后端端口时，用 `-H`：

```powershell
ii relay --public https://relay.example.com:8443 -H 9443
```

发送端指定 relay：

```powershell
ii send .\video.mp4 --relay https://服务器公网IP:8443 -k
```

`-k` 表示接受自签证书，并把该策略带进 ticket；接收方无需安装证书或配置 relay。首次连接仍可能遭遇中间人替换。

已有域名和 PEM 证书时，使用手工证书模式：

```powershell
ii relay --tls relay.example.com -H 8443 --cert D:\certs\fullchain.pem --key D:\certs\privkey.pem
ii send .\video.mp4 --relay https://relay.example.com:8443
```

手工证书模式不带 `-k`，客户端使用系统正常 TLS 校验。两种 `--relay` 都只走 HTTPS relay，不尝试 UDP 或直连；完整端口、状态路径和安全边界见 [ii.md](ii.md)。

## 详细手册

完整命令、端口职责、TLS 来源、配置路径、故障排查和底层对应关系都写在 [ii.md](ii.md)。

## 变更记录

版本变更见 [CHANGELOG.md](CHANGELOG.md)。英文版本见 [CHANGELOG.en.md](CHANGELOG.en.md)。

## 版本

当前版本由 Git tag 管理。仓库内已使用 `v0.1.11`。

## 许可证

本项目使用 MIT License。你可以自由使用、修改和分发，详见 [LICENSE](LICENSE)。
