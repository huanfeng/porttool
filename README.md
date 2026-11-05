# PortTool

一个托盘常驻的端口转发工具，把三类转发放在同一个界面里管理：

- **串口共享**：把本机串口通过 TCP 共享出去，WindTerm / XShell 等终端、局域网内其它设备以及 AI Agent
  可以**同时**使用同一个串口，主要用于设备调试时抓取日志；
- **SSH 转发**：托管系统 `ssh` 进程，建立本地（`-L`）或远程（`-R`）端口转发，断线后自动重连；
- **TCP 转发**：把本机一个端口的流量转发到另一个地址。

组成：

- `crates/porttool-core`：服务核心（串口、日志、Telnet 终端、SSH / TCP 转发、HTTP 接口）
- `crates/porttoolw`：托盘常驻的图形界面
- `crates/porttool`：命令行工具，供脚本和 AI 使用；也可以列出本机串口、无界面运行服务

设计见 [docs/DESIGN.md](docs/DESIGN.md)，AI 接入见 [docs/AI.md](docs/AI.md)。

## 构建

```sh
cargo build --release
# 产物：target/release/porttoolw.exe、target/release/porttool.exe
```

SSH 转发需要系统里已安装 OpenSSH 客户端（Windows 10+ 自带）。

## 使用

1. 运行 `porttoolw.exe`，托盘区出现图标，主窗口自动打开。
2. 点右上角「新建」，选择串口 / SSH / TCP，填写后保存，立即生效。
   - 串口：有 USB 序列号的设备按序列号绑定，换 USB 口、COM 号变化后也能自动认回来；
     在 WindTerm / XShell 中新建 **Telnet** 会话，主机填 `127.0.0.1`（局域网其它机器填本机 IP），
     端口填该串口的 TCP 端口，终端编码请和设备输出一致；
   - SSH：填 SSH 服务器（`user@host` 或 `~/.ssh/config` 里的别名）、监听端口和目标地址；
     认证走系统 ssh（密钥 / ssh-agent），不会弹出密码输入；
   - TCP：填监听地址和目标地址，如 `0.0.0.0:8080` → `192.168.1.100:80`。
3. 概览页可以搜索、按类型筛选，行内开关启停规则；有规则失败时页面顶部会出现告警条。
4. AI 或脚本使用 `porttool`：

   ```sh
   porttool list                      # 所有规则及状态
   porttool tail esp32 -n 100
   porttool send esp32 reboot --expect "boot done" --timeout 20
   ```

关闭主窗口后程序继续在托盘运行；退出请用托盘菜单的「退出」。托盘菜单里可以开启开机自启。

### 文件位置

| 内容 | 位置 |
|---|---|
| 配置文件 | `%APPDATA%\porttool\config.toml`（`--config` 可指定），示例见 [config.example.toml](config.example.toml) |
| 串口日志 | 配置文件旁的 `logs\<别名>\<别名>-YYYY-MM-DD.log` |
| 程序日志 | 配置文件旁的 `porttool.log` |

### 命令行参数

```
porttoolw [--config <path>] [--autostart]
  --autostart   只显示托盘，不打开主窗口（开机自启使用）
```

porttoolw 是 GUI 程序，命令行功能都在 `porttool` 里：

```
porttool list                        所有转发规则及状态
porttool ports                       串口详情（波特率、端口、客户端、日志序号）
porttool enable|disable <name>       启动 / 停止 SSH、TCP 规则
porttool devices --local             列出本机串口及 USB 序列号（不需要服务在运行）
porttool serve [--config <path>]     不启动界面，在前台运行服务（日志输出到控制台）
porttool tail|log|grep|wait|follow|send <name> ...   读取串口日志、向设备发送数据
```

环境变量：`PORTTOOL_HOST`（服务地址，默认 `127.0.0.1:7000`）、`PORTTOOL_TOKEN`（访问 token）。

## 安全

服务默认监听 `0.0.0.0`，局域网内任何设备都可以读写串口、使用 TCP 转发。需要限制时：

- 设置页（或配置里的 `[server] bind = "127.0.0.1"`）：只允许本机访问；
- `[server] token = "..."`：HTTP 接口需要 token。串口终端端口（Telnet/Raw）和 TCP 转发不支持认证。

HTTP 接口会拒绝带 `Origin` 头的请求和非 IP / 本机名的 `Host`，防止本机浏览器中的网页借道访问。
