use std::{env, process::Stdio};

use anyhow::Result;
use common::main_loop::daemon;
use tokio::{self};

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr = "127.0.0.1:1234";

    let mut args: Vec<String> = env::args().collect();

    if let Some(arg) = args.get(1) {
        match arg.as_str() {
            "daemon" => {
                let current_exe = env::current_exe()?;
                let child = std::process::Command::new(current_exe)
                    .arg("_daemon_internal")
                    .arg(args.get(2).unwrap_or(&server_addr.to_string()))
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()?;
                println!("守护进程已启动 pid: {}", child.id());
            }
            "_daemon_internal" => {
                daemon(server_addr).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
