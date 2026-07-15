<p align="center">
  <img src="logo.svg" alt="ii logo" width="96" height="96">
</p>

<h1 align="center">ii</h1>

<p align="center">一个跨平台文件传输 CLI。</p>

---

它只有一个对外品牌：`ii`。用户只需要记 `send`、`recv`、`relay`、`doctor`、`version`。

## 快速开始

发送文件：

```powershell
ii send .\video.mp4
```

接收文件：

```powershell
ii recv ii1k7v...x9a
```

发送目录：

```powershell
ii send .\my-folder
```

保持发送端不退出：

```powershell
ii send .\my-folder -t
```

## 核心用法

`ii send` 支持文件、目录和 stdin。默认只成功发送一次；`-t` 会让发送端继续保持可用，直到你手动退出。

`ii recv` 只需要 ticket。它默认会自动处理三种情况：同名同内容直接跳过，文件未传完就续传，内容不同就覆盖。目录会按原目录结构还原到目标位置。

`ii relay` 用来启动 relay 服务。默认配置路径按平台决定：Windows 读取 `ii.exe` 同目录下的 `relay.toml`，Linux/macOS/其他 Unix-like 使用 `/etc/ii/relay.toml`。没有配置时会先生成默认文件再启动。

`ii doctor` 用来排查本机网络、relay、端口和权限问题。`ii version` 输出当前版本。

## 常见场景

同事之间临时传一个文件：

```powershell
ii send .\report.pdf
ii recv ii1k7v...x9a
```

发送端和接收端的实际样子：

![发送端截图](screenshot/发送.png)

![接收端截图](screenshot/接收.png)

传一个大目录给另一台机器：

```powershell
ii send .\project
ii recv ii1k7v...x9a -o D:\Downloads
```

把 stdin 直接传过去：

```powershell
tar czf - .\project | ii send --name project.tar.gz
ii recv ii1k7v...x9a --stdout > project.tar.gz
```

局域网优先，不走公网 relay：

```powershell
ii send .\file.zip --local
ii recv ii1k7v...x9a --local
```

排查为什么慢：

```powershell
ii recv ii1k7v...x9a --trace
ii recv ii1k7v...x9a --local --trace
```

## Relay

默认端口：

- HTTP: `80`
- HTTPS: `443`
- QUIC: `7842`
- metrics: `9090`，默认关闭

如果 `80/443` 已经被 Nginx 占用，可以把 `ii relay` 放到非标准端口，再让前置代理转发。

TLS 生产模式主要走 ACME 自动签发；开发模式可以用 `--dev` 走 plain HTTP。

## 详细手册

完整命令、端口职责、TLS 来源、配置路径、故障排查和底层对应关系都写在 [ii.md](ii.md)。

## 版本

当前版本由 Git tag 管理。仓库内已使用 `v0.1.1`。
