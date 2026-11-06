# PortTool 设计文档

一个托盘常驻的端口转发工具：把本机串口共享给局域网（多个使用方可以**同时**读写同一个串口），
并统一管理 SSH 与 TCP 端口转发。主要场景是设备调试：抓取串口日志、通过跳板机访问内网服务。

## 1. 目标与非目标

**目标**

- 一个后台服务，统一管理三类转发规则：串口共享、SSH 转发、TCP 转发。
- 串口：独占物理串口并分发给多个使用方，所有使用方都可以写入；用别名标识设备并绑定 USB 序列号
  （COM 号变了也能认回设备）；支持热插拔，设备断开后自动重连，终端连接保持不断；
  每个串口一个固定的 Telnet / Raw TCP 端口。
- SSH：托管系统 `ssh` 进程，支持本地（-L）和远程（-R）转发；只对瞬时网络故障自动退避重连。
- TCP：纯 tokio 的双向转发，限制最大并发连接数。
- 面向 AI：纯文本日志文件 + 文本优先的 HTTP 接口 + `porttool` 命令行。
- GUI：基于 windui 的托盘常驻程序。

**不做**：RFC2217、虚拟 COM 口、SSH 动态转发（-D / SOCKS）、ssh_config 导入、
SSH / TCP 的流量统计、断开单个终端、认证以外的安全机制。

## 2. 架构

```
porttoolw.exe（托盘常驻；无界面运行用 `porttool serve`）
├─ UI 线程：windui 事件循环（托盘 + 按需打开的主窗口）
│      定时器读取服务快照 → 写入信号；用户操作 → 修改配置文件 → 让服务重新加载
└─ 服务运行时（tokio，后台线程）—— crate `porttool-core`
   └─ Service：按配置管理所有规则
      ├─ 串口规则 → PortHub：每个串口一个
      │   ├─ 串口线程：查找设备 → 打开 → 阻塞读；出错就关闭并重新查找
      │   ├─ 写入线程：合并所有客户端写入的数据，按顺序写到串口
      │   ├─ 行切分器（RX / TX 各一个）：按换行或空闲超时切行
      │   ├─ 内存环形缓冲：最近 N 行，每行带递增的 seq
      │   ├─ 日志文件：按天滚动
      │   └─ Terminal 服务：每个串口一个 TCP 端口（telnet / raw）
      ├─ SSH 规则 → SshForwarder：监督任务启动 ssh、解析 stderr、退避重连
      ├─ TCP 规则 → TcpForwarder：accept + copy_bidirectional，原子计数限制连接数
      └─ HTTP 服务：一个端口（默认 7000）

porttool.exe（控制台程序，通过 HTTP 和服务通信）
```

`Service::reload` 比较新旧配置：没变的规则保持运行（终端连接不断），变了的重建；
串口重建时沿用旧缓冲，seq 连续、调用方的游标不会失效。

串口数据有两条路径：

- **原始字节路径**：串口 → 广播 → 各个终端连接。不加时间戳，不做清洗，和直连串口时完全一样。
- **文本路径**：串口 → 解码 → 行切分 → 去掉 ANSI 转义 → 环形缓冲 + 日志文件。供 AI 和 HTTP 使用。

## 3. 日志格式

```
2026-10-23 14:57:01.123 [RX] I (312) wifi: connected, ip=192.168.1.50
2026-10-23 14:57:03.004 [TX] reboot
2026-10-23 14:57:03.500 [SYS] device disconnected
```

- `RX`：设备发出的数据；`TX`：写往设备的数据；`SYS`：服务自身的事件（连接、断开等）。
- 切行规则：RX 遇到 `\n` 切行，遇到 `\r` 丢弃；超过 100ms 没有新数据时，把未结束的内容作为一行写出
  （例如 `login: ` 这种提示符）。TX 遇到 `\r` 或 `\n` 切行，空闲超时为 1s
  （因为终端是逐字符发送的）。单行最长 4096 字节，超过就强制切行。
- 编码：可以按串口配置 `utf-8`（默认）或 `gbk`。无法解码的字节，UTF-8 下表示为 `\xNN`，GBK 下表示为 `�`。
- 文件位置：`<log_dir>/<name>/<name>-YYYY-MM-DD.log`。

## 4. 配置文件

位置：`%APPDATA%\porttool\config.toml`，可以用 `--config <path>` 指定其它路径。完整示例见
[config.example.toml](../config.example.toml)。

```toml
[server]
bind = "0.0.0.0"          # HTTP 和串口 Terminal 服务的监听地址
http_port = 7000
# log_dir = "D:/porttool-logs"  # 默认是配置文件所在目录下的 logs/
# token = "secret"        # 设置后 HTTP 请求需要带上 token
buffer_lines = 20000      # 每个串口在内存中保留的行数

# 每条转发规则是一个 [[rule]]，type 决定类型：serial | ssh | tcp
[[rule]]
type = "serial"
name = "esp32"            # 名称：只能用字母、数字、- 和 _，不区分大小写且不能重复
serial_number = "A50285BI"   # USB 序列号，优先匹配
# com = "COM5"            # 没有序列号时按 COM 号匹配
baud = 115200
tcp_port = 7001
mode = "telnet"           # telnet | raw
encoding = "utf-8"        # utf-8 | gbk
enabled = true            # false 时保留配置但不运行

[[rule]]
type = "ssh"
name = "gitlab"
ssh_target = "ops@jump.example.com"   # user@host 或 ssh_config 里的别名
# ssh_port = 22
# identity_file = "C:/Users/me/.ssh/id_ed25519"
direction = "local"       # local = -L（本机监听）；remote = -R（远端监听）
local_port = 8443
remote_host = "gitlab.internal"
remote_port = 443
auto_reconnect = true

[[rule]]
type = "tcp"
name = "web-ui"
listen = "0.0.0.0:8080"
target = "192.168.5.50:80"
max_connections = 1000
```

串口匹配规则：配置了 `serial_number` 就只按序列号匹配；否则按 `com` 匹配。
校验：名称合法且唯一；本机监听端点不能冲突——HTTP、串口终端（跟随 `server.bind`）、SSH 本地转发（`127.0.0.1`）、TCP `listen`
同端口且地址重叠（相同，或同一地址族的通配地址）时报错并指出冲突双方，SSH 远程转发（-R）的端口在服务器侧不算，已停用的规则不占端口；
SSH 的目标 / 私钥路径 / 远端主机不能以 `-` 开头或带空白（会原样传给 ssh 命令行）；
TCP 的监听和目标必须是 `IP:端口`。

## 5. 串口 Terminal 服务

- 每个串口在 `bind:tcp_port` 上监听，允许多个客户端同时连接。
- `telnet` 模式：连接时发送 `IAC WILL ECHO`、`IAC WILL SGA`、`IAC WILL BINARY` 和 `IAC DO BINARY`，
  让终端进入逐字符模式；发出的数据把 `0xFF` 转义为 `0xFF 0xFF`；收到的数据中去掉 IAC 命令，
  并把 `\r\0` 还原为 `\r`。
- `raw` 模式：原样透传。
- 连接建立时发送一行提示，例如 `[porttool] esp32 (COM5, 115200) connected`。设备断开或重连时，
  也向所有终端推送一行提示。

## 6. SSH 转发

- 命令：`ssh -v -N -o ExitOnForwardFailure=yes -o BatchMode=yes -o ServerAliveInterval=30
  -o ServerAliveCountMax=3 -o ConnectTimeout=15 [-p] [-i] -L|-R 127.0.0.1:<本地端口>:<目标>:<目标端口> <服务器>`。
  认证全部交给系统 ssh（BatchMode，不会等待输入）。
- 就绪判断：读 stderr，`-L` 等 `Local forwarding listening on`，`-R` 等 `remote forward success for`。
- 重连：只有诊断里出现 connection reset / timed out / refused / no route / could not resolve 等瞬时网络故障、
  且没有 permission denied / host key / address already in use 等需要用户处理的错误时才重连；
  退避 2、4、8…最多 30 秒，稳定运行 30 秒后重新计数。
- 停止：中止监督任务，`kill_on_drop` 结束 ssh 进程。

## 7. TCP 转发

- 每个连接先连目标（5 秒超时），成功后 `copy_bidirectional`；
- 最大并发连接数用 CAS 循环原子地占位，连接失败或结束时通过 RAII guard 释放。

## 8. HTTP 接口

默认返回 `text/plain; charset=utf-8`。加上 `?format=json` 返回 JSON。设置了 token 时，需要带
`Authorization: Bearer <token>` 头，或者 `?token=<token>` 参数。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/` | 使用说明（Markdown），给 AI 读 |
| GET | `/rules` | 所有规则（串口、SSH、TCP，含已停用的）及状态；JSON 带 `endpoint`（串口当前 COM 号）与 `attempt`（SSH 第几次重连） |
| POST | `/rules/{name}/enable` `/disable` | 启用 / 停用规则（串口、SSH、TCP 都适用，持久化到配置） |
| GET | `/ports` | 串口详情：别名、状态、TCP 端口、客户端数量、最新 seq |
| GET | `/devices` | 本机串口设备 |
| GET | `/ports/{name}/tail?lines=200` | 最近 N 行 |
| GET | `/ports/{name}/log?since=<seq>&limit=1000` | 增量读取，返回 seq 大于 since 的行；响应头带 `X-Next-Seq` |
| GET | `/ports/{name}/grep?pattern=<regex>&limit=200` | 在内存缓冲中搜索 |
| GET | `/ports/{name}/wait?pattern=<regex>&timeout=30` | 等待新出现的匹配行，期间收到的行一并返回 |
| GET | `/ports/{name}/stream` | 持续推送新日志行（`curl -N`） |
| POST | `/ports/{name}/send?expect=<regex>&timeout=5&eol=crlf` | 发送请求体；可选等待匹配 |

- `seq` 是每个串口内从 1 开始递增的行号（服务重启后重新计数）。如果 `since` 比当前最大的 seq 还大
  （说明服务重启过），就从缓冲区最早的一行开始返回。
- `send` 的 `eol` 参数决定在请求体末尾追加什么：`none`、`cr`、`lf`、`crlf`，默认是 `crlf`。
- `wait` 和 `send?expect=` 默认只用 RX 行匹配（`dir=any` 时所有方向都参与），返回期间所有方向的行。
  超时返回 HTTP 408，串口被停止返回 503，期间收到的行仍然放在响应体里。
- `log` 的游标之后若有行已被挤出内存缓冲，响应开头会有一行 `-- gap: 第 X..Y 行…`（JSON 中为 `gap` 字段）。
- 安全：拒绝带 `Origin` 头的请求和非 IP / 本机名的 `Host`。

## 9. porttool 命令行

```
porttool [--host <addr>] [--token <t>] [--json] <命令>
  list                                  所有规则及状态
  ports                                 串口详情
  enable|disable <name>                 启用 / 停用规则（写入配置，持久化）
  tail   <name> [-n 200]                最近 N 行
  log    <name> [--since <seq>]         增量读取（输出末尾会打印 next seq）
  grep   <name> <regex>                 搜索
  wait   <name> <regex> [--timeout 30]  等待某行出现
  follow <name>                         持续输出
  send   <name> <text> [--expect <regex>] [--timeout 5] [--eol crlf]
  devices [--local]                     服务所在机器的串口；--local 直接枚举本机，不需要服务
  doc                                   显示服务端的接口说明
  serve  [--config <path>]              不启动界面，在前台运行服务
```

porttoolw 是 GUI 子系统程序，没有控制台输出，所以"列出串口""无界面运行服务"这类命令行功能放在
porttool（控制台程序）里。`serve` 与托盘程序共用同一份配置，两者不要同时运行。
`--host` 默认是 `127.0.0.1:7000`，也可以用环境变量 `PORTTOOL_HOST` 指定。

## 10. 图形界面

- **外壳**：左侧导航（概览 / 串口共享 / SSH 转发 / TCP 转发 / 设置，带数量徽章）、顶部状态摘要
  （`4/7 在线 · 2 需处理`）、底部状态栏（HTTP 接口地址，点一下复制；打开日志目录、配置文件等都在「设置」页）。
- **概览**：4 张统计卡片、失败规则的告警条、可搜索和按类型筛选的规则表格（行内启停开关、编辑 / 删除）。
- **串口页**：左侧规则卡片，右侧选中串口的详情（连接命令、参数、收发字节）。
- **SSH 页**：每条规则一张卡片，展示「本机监听 → SSH 服务器 → 目标」。
- **TCP 页**：规则表格。
- **设置**：开机自启、主题（亮 / 暗 / 跟随系统）、服务监听地址 / HTTP 端口 / token / 缓冲 / 日志目录。
- **对话框**：串口、SSH、TCP 的添加 / 编辑，保存前经 `Config::validate` 校验，端口占用会提示；
  删除需要确认。
- 修改配置的基准永远是磁盘上的最新文件；配置文件解析失败时界面禁止保存，避免用空配置覆盖原文件。
- 颜色全部使用主题角色，亮 / 暗主题运行期切换。

截图验证：`porttoolw --config x.toml --screenshot out.png`；环境变量
`PORTTOOL_SCREENSHOT_STATE="page=2;theme=1;dialog=ssh"` 可指定页面、主题和打开的对话框。
