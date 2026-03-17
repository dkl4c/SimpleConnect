use std::io::Read;
use std::{env, io::Write};

use anyhow::Result;
use common::main_loop::{DAEMON_LOCK, daemon};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tokio::{self, fs::File};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    server_addr: String,
    port: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let content =
        tokio::fs::read_to_string("/home/anju/.config/simple_connect/config.toml").await?;
    let config: Config = toml::from_str(&content)?;
    let server_addr = format!("{}:{}", config.server_addr, config.port);

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Err(anyhow::anyhow!("缺少命令参数"));
    }

    if let Some(arg) = args.get(1) {
        match arg.as_str() {
            "daemon" => {
                let current_exe = env::current_exe()?;
                let child = std::process::Command::new(current_exe)
                    .arg("_daemon_internal")
                    .arg(args.get(2).unwrap_or(&server_addr.to_string()))
                    // .stdin(Stdio::null())
                    // .stderr(Stdio::null())
                    // .stdout(Stdio::null())
                    .spawn()?;
                println!("尝试启动守护进程 pid: {}", child.id());
            }
            "_daemon_internal" => {
                daemon(&server_addr).await?;
            }
            "kill" => {
                let mut pid = String::new();
                let mut daemon_lock = std::fs::File::open(DAEMON_LOCK)?;
                daemon_lock.read_to_string(&mut pid)?;
                let pid = pid.trim().parse::<String>()?;
                println!("尝试停止守护进程，PID: {}", pid);

                let status = std::process::Command::new("kill").arg(&pid).status()?;
                if status.success() {
                    println!("成功退出守护进程, PID: {}", pid);
                }

                let mut daemon_lock = std::fs::OpenOptions::new().write(true).open(DAEMON_LOCK)?;
                daemon_lock.set_len(0)?;
                daemon_lock.flush()?
            }
            "attach" => {}
            _ => {}
        }
    }

    Ok(())
}
