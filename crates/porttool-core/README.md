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
└── forwarding/
    ├── serial.rs      # 串口转发实现
    ├── ssh.rs         # SSH 转发实现
    └── tcp.rs         # TCP 转发实现
```

## 使用

```rust
use porttool_core::{Config, Service};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load("config.toml")?;
    let service = Service::start(config, "config.toml".into()).await?;
    
    // 列出所有规则状态
    let rules = service.list_rules().await;
    for rule in rules {
        println!("{}: {:?}", rule.name, rule.state);
    }
    
    // 启用规则
    service.enable_rule("esp32").await?;
    
    // 等待退出信号
    tokio::signal::ctrl_c().await?;
    service.shutdown().await;
    Ok(())
}
```

## 配置格式

参见根目录的 `config.example.toml`。

## 开发状态

- [x] Phase 1: 项目结构与配置模块
- [ ] Phase 2: 串口转发实现（迁移 comhub-core）
- [ ] Phase 3: TCP 转发实现
- [ ] Phase 4: SSH 转发实现（迁移 porttool）
- [ ] Phase 5: HTTP API
- [ ] Phase 6: CLI 工具
- [ ] Phase 7: 跨平台测试

## License

MIT OR Apache-2.0
