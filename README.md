# 自动连接无线 ADB

这个服务会启动一个本地 HTTP API, 用来转发无线 ADB 和 `scrcpy` 相关操作.

发现方式分两层:

- 默认启用 `mdns`
- 如果在编译时注入了 `lnd` 相关环境变量, 会额外启用第二层 `lnd` announce

`lnd` 不是运行时配置项. 这个应用启动时不会额外要求你传 `lnd` 参数.

## 安装

```shell
git clone https://github.com/azazo1/auto_adb_wl_server.git
cd auto_adb_wl_server
cargo install --path .
```

然后设置 `auto_adb_wl_server` 为开机自启动.

## 启动

默认监听端口是 `21300`:

```shell
auto_adb_wl_server
```

也可以显式指定端口:

```shell
auto_adb_wl_server --port 21300
```

## 网络切换恢复

服务会自动监听网络接口, IP 地址和硬件地址的变化. 连续变化停止 2 秒后, 顺序执行 `adb kill-server` 和 `adb start-server`, 并重新发布 mDNS 服务.

- 启动时的首次网络快照不会触发重启, 无实际变化的通知也会忽略.
- 应用内的 ADB 请求会等待正在进行的重启完成, 单条 ADB 命令最多执行 30 秒.
- 重启成功或失败会输出日志. 重启失败后不会无限重试.
- ADB server 由本机工具共用, 重启会短暂中断现有 ADB 和 scrcpy 连接. 本服务不会保存并逐一重连此前的 ADB 目标, scrcpy 仍使用已有的重连逻辑.
- 检测依赖系统的接口变化通知, 不保证覆盖接口和地址完全不变的纯路由或 Wi-Fi 切换. 自动重启也不能解决设备确实不在可达局域网内的问题.

## lnd 接入

如果你只需要 `mdns`, 不需要做任何额外配置.

如果你还想启用第二层 `lnd` 发现, 需要在编译时注入下面两个环境变量:

- `AUTO_ADB_WL_LND_BASE_URL`
- `AUTO_ADB_WL_LND_BEARER_TOKEN`

仓库里提供了示例文件 [\.env.example](/Users/azazo1/pjs/rust/auto_adb_wl_server/.env.example:1).

最简单的方式是先准备 `.env`:

```env
AUTO_ADB_WL_LND_BASE_URL=http://127.0.0.1:8765
AUTO_ADB_WL_LND_BEARER_TOKEN=dev-token
```

然后再编译:

```shell
cargo install --path .
```

如果编译时没有注入 `AUTO_ADB_WL_LND_BASE_URL`, 运行时就只会使用 `mdns`, 不会启用 `lnd`.

## 说明

- `lnd` 依赖来源是 GitHub 仓库: [azazo1/lnd](https://github.com/azazo1/lnd)
- `lnd` 的 `node_id` 会自动持久化到系统状态目录
- `lnd` 会在启动时自动解析本机 announce 地址和 `reachability_scopes`
