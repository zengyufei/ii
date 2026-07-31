# ii WebRTC 局域网直传说明

`ii webrtc` 在 `0.0.0.0` 的随机端口启动临时局域网浏览器直传房间。打开同一 URL 的浏览器互相发送文件时，文件通过 WebRTC DataChannel 直接在两台浏览器之间传输；`ii` 只转发 HTTP 信令，不接收文件，也不写入启动机器磁盘。

## 命令

```powershell
ii webrtc
ii webrtc --token A1b2C3d4E5f6G7h8
```

启动后，终端会在主 IPv4 LAN URL 上方显示二维码，并在 `other:` 下列出其他物理和虚拟网卡的 IPv4 URL。所有设备打开同一个 URL 后自动得到临时设备编号；选择目标设备、等待状态显示 `Connected to Device N`，再选择一个或多个独立文件发送。接收端完成后自动触发浏览器下载。

按 `Ctrl+C` 停止房间。成员 30 秒没有 HTTP 活动会从房间中移除。

## 浏览器要求

页面加载时会创建本地 DataChannel 并等待 ICE host candidate，确认浏览器实际可用 WebRTC 后才加入房间。浏览器禁用或阻止 WebRTC 时，页面显示：

```text
WebRTC unavailable
WebRTC is disabled or blocked in this browser. Enable WebRTC and reload.
```

此时必须在浏览器或其策略、扩展中重新启用 WebRTC 后刷新页面。仅能看到其他设备编号不代表浏览器可建立 DataChannel；设备列表由 HTTP 信令服务提供。

## 局域网范围和限制

只交换局域网 host candidates，`iceServers` 为空，不使用公网 STUN/TURN，也没有公网回退。因此客户端隔离、访客 Wi-Fi、跨网段策略、防火墙、禁止 P2P 或浏览器关闭 WebRTC 都会导致连接失败。

DataChannel 是可靠、有序的。每个文件以 16 KiB 分块传输，并在浏览器缓冲区达到阈值时回压。接收端会先把单个完整文件聚合到浏览器内存，再创建下载；不要传超过接收设备可用内存的文件。

不支持目录、断点续传、服务端保存文件或 CLI 与浏览器间直接传输。

## 路径令牌

默认页面和信令接口在根路径下：

```text
/
```

传入 `--token <value>` 后，页面和全部信令接口固定在：

```text
/<value>/
```

令牌只能为 16 到 128 个 ASCII 字母、数字、`-` 或 `_`。遗漏、写错或使用其他路径均返回 `404`。令牌是 URL 路径访问令牌，不是加密或身份认证；任何持有完整 URL 的设备都可以加入房间。

`ii webrtc` 不接受位置参数、`--path` 或 `-p`；`-p` 仍只用于 WebDAV、FTP、SFTP 的便携 ticket。
