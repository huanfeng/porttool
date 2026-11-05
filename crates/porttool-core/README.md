# porttool-core

PortTool 核心服务库，提供串口、SSH、TCP 端口转发功能。

## 特性

- 🔌 **串口转发**：通过 TCP 共享串口，支持多客户端并发访问
- 🔐 **SSH 转发**：调用系统 OpenSSH 实现本地/远程端口转发
- 🌐 **TCP 转发**：纯 tokio 实现的 TCP 端口转发
- 🔄 **热重载**：修改配置无需重启服务
- 📊 **HTTP API**：提供统一的管理和数据接口
- 🖥️ **跨平台**：支持 Windows、Linux、macOS

## 架构

```
porttool-core/
├── config.rs          # 配置文件读写与校验（TOML）
├── service.rs         # 服务总入口，管理所有转发规则
├── port.rs            # 串口：PortHub（IO 线程、热插拔、缓冲、日志）
├── terminal.rs        # 串口的 Telnet / Raw 终端服务
├── http.rs            # HTTP 接口
├── buffer.rs / logfile.rs / text.rs / devices.rs
└── forwarding/
    ├── ssh.rs         # SSH 转发实现
    └── tcp.rs         # TCP 转发实现
```

## 使用

```rust
use porttool_core::{Config, Service};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load("config.toml".as_ref())?;
    let service = Service::start(config, "config.toml".into(), |_event| {}).await?;
    
    // 列出所有规则状态
    let rules = service.list_rules().await;
    for rule in rules {
        println!("{}: {:?}", rule.name, rule.state);
    }
    
    // 启动 / 停止 SSH、TCP 规则
    service.enable_rule("dev-proxy").await?;
    
    // 等待退出信号
    tokio::signal::ctrl_c().await?;
    service.shutdown().await;
    Ok(())
}
```

## 配置格式

参见根目录的 `config.example.toml`。

## 开发状态

- [x] 配置模块（`[[rule]]`：serial / ssh / tcp）
- [x] TCP 转发
- [x] SSH 转发（托管系统 ssh，瞬时故障自动重连）
- [x] 串口共享（PortHub、热插拔、日志、Telnet 终端）
- [x] HTTP 接口（`/rules`、`/ports/...`）
- [x] 命令行（`porttool`）与托盘 GUI（`porttoolw`）

## License

MIT OR Apache-2.0
