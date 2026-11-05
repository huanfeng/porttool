use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "porttool", version, about = "多协议端口转发工具")]
struct Cli {
    /// 配置文件路径
    #[arg(long, env = "PORTTOOL_CONFIG")]
    config: Option<PathBuf>,

    /// HTTP API 认证 token
    #[arg(long, env = "PORTTOOL_TOKEN")]
    token: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 前台运行服务（不启动 GUI）
    Serve {
        /// 配置文件路径
        #[arg(short, long)]
        config: Option<PathBuf>,
    },

    /// 列出所有规则及状态
    List {
        /// 服务地址
        #[arg(long, default_value = "http://127.0.0.1:7000")]
        server: String,
    },

    /// 启用规则
    Enable {
        name: String,
        /// 服务地址
        #[arg(long, default_value = "http://127.0.0.1:7000")]
        server: String,
    },

    /// 停用规则
    Disable {
        name: String,
        /// 服务地址
        #[arg(long, default_value = "http://127.0.0.1:7000")]
        server: String,
    },

    /// 列出本机串口（不需要服务运行）
    Devices {
        #[arg(long)]
        local: bool,
    },

    /// 读取串口日志（需要服务运行）
    Tail {
        name: String,
        #[arg(short = 'n', default_value = "100")]
        lines: usize,
        #[arg(short, long)]
        follow: bool,
        #[arg(long, default_value = "http://127.0.0.1:7000")]
        server: String,
    },

    /// 向串口发送数据
    Send {
        name: String,
        data: String,
        #[arg(long)]
        expect: Option<String>,
        #[arg(long, default_value = "10")]
        timeout: u64,
        #[arg(long, default_value = "http://127.0.0.1:7000")]
        server: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config } => {
            let config_path = config
                .or(cli.config)
                .unwrap_or_else(|| porttool_core::Config::default_path());
            serve(config_path).await
        }
        Commands::List { server } => {
            list_rules(&server, cli.token.as_deref()).await
        }
        Commands::Enable { name, server } => {
            enable_rule(&server, &name, cli.token.as_deref()).await
        }
        Commands::Disable { name, server } => {
            disable_rule(&server, &name, cli.token.as_deref()).await
        }
        Commands::Devices { local } => {
            if local {
                list_local_devices().await
            } else {
                anyhow::bail!("请使用 --local 列出本机串口")
            }
        }
        Commands::Tail { name, lines, follow, server } => {
            if follow {
                anyhow::bail!("--follow 功能尚未实现")
            }
            tail_serial(&server, &name, lines, cli.token.as_deref()).await
        }
        Commands::Send { name, data, expect, timeout, server } => {
            send_serial(&server, &name, &data, expect.as_deref(), timeout, cli.token.as_deref()).await
        }
    }
}

async fn serve(config_path: PathBuf) -> anyhow::Result<()> {
    println!("PortTool 服务启动中...");
    println!("配置文件: {}", config_path.display());

    let config = porttool_core::Config::load(&config_path)?;
    let service = porttool_core::Service::start(config.clone(), config_path, |_| {}).await?;

    println!("✓ 服务已启动");
    println!("  HTTP API: http://{}:{}", config.server.bind, config.server.http_port);
    println!("  配置规则: {} 条", config.rules.len());
    println!("\n按 Ctrl+C 退出");

    tokio::signal::ctrl_c().await?;
    println!("\n正在关闭服务...");
    service.shutdown().await;
    println!("服务已停止");
    Ok(())
}

async fn list_rules(server: &str, token: Option<&str>) -> anyhow::Result<()> {
    println!("正在连接服务: {}", server);
    println!("\n提示: 该功能需要 HTTP API 实现（Phase 5）");
    Ok(())
}

async fn enable_rule(server: &str, name: &str, token: Option<&str>) -> anyhow::Result<()> {
    println!("启用规则: {}", name);
    println!("\n提示: 该功能需要 HTTP API 实现（Phase 5）");
    Ok(())
}

async fn disable_rule(server: &str, name: &str, token: Option<&str>) -> anyhow::Result<()> {
    println!("停用规则: {}", name);
    println!("\n提示: 该功能需要 HTTP API 实现（Phase 5）");
    Ok(())
}

async fn list_local_devices() -> anyhow::Result<()> {
    println!("扫描本机串口...\n");

    match serialport::available_ports() {
        Ok(ports) => {
            if ports.is_empty() {
                println!("未找到串口设备");
            } else {
                println!("发现 {} 个串口:\n", ports.len());
                for port in ports {
                    println!("  {}", port.port_name);
                    if let serialport::SerialPortType::UsbPort(info) = &port.port_type {
                        if let Some(serial) = &info.serial_number {
                            println!("    USB 序列号: {}", serial);
                        }
                        if let Some(product) = &info.product {
                            println!("    产品: {}", product);
                        }
                        if let Some(manufacturer) = &info.manufacturer {
                            println!("    制造商: {}", manufacturer);
                        }
                    }
                    println!();
                }
            }
        }
        Err(e) => {
            anyhow::bail!("枚举串口失败: {}", e);
        }
    }
    Ok(())
}

async fn tail_serial(server: &str, name: &str, lines: usize, token: Option<&str>) -> anyhow::Result<()> {
    println!("读取串口日志: {} (最后 {} 行)", name, lines);
    println!("\n提示: 该功能需要 HTTP API 实现（Phase 5）");
    Ok(())
}

async fn send_serial(
    server: &str,
    name: &str,
    data: &str,
    expect: Option<&str>,
    timeout: u64,
    token: Option<&str>,
) -> anyhow::Result<()> {
    println!("向串口发送数据: {}", name);
    println!("  数据: {}", data);
    if let Some(pattern) = expect {
        println!("  等待响应: {} (超时 {}s)", pattern, timeout);
    }
    println!("\n提示: 该功能需要 HTTP API 实现（Phase 5）");
    Ok(())
}
