<p align="center">
  <img src="logo.svg" alt="ii logo" width="96" height="96">
</p>

<h1 align="center">ii</h1>

<p align="center">一个跨平台文件传输 CLI。</p>

---

`ii` 面向临时传文件：

- 发送端默认一次性发送，成功后退出
- 复杂网络下会短时间尝试寻找可用通路
- 接收端默认断点续传
- 已存在且 MD5 相同的文件会直接跳过
- 文件夹可以直接发送

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

启动 relay：

```powershell
ii relay
```

默认端口：

- HTTP: `80`
- HTTPS: `443`
- QUIC: `7842`
- metrics: `9090`，默认关闭

如果 `80/443` 已经被 Nginx 占用，可以把 `ii relay` 放到非标准端口，再让前置代理转发。`7842/udp` 是独立 QUIC 端口，不能靠普通 HTTP 反代替代。

TLS 生产模式主要走 ACME 自动签发；开发模式可以用 `--dev` 走 plain HTTP。

## 详细手册

完整命令、端口职责、TLS 来源、配置路径、故障排查和底层对应关系都写在 [ii.md](ii.md)。

## 版本

当前版本由 Git tag 管理。仓库内已使用 `v0.1.2`。
