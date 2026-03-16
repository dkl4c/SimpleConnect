use anyhow::{Ok, Result};
use tokio::{io::AsyncReadExt, net::TcpListener};

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr = "127.0.0.1:1234";
    let server = TcpListener::bind(server_addr).await?;
    println!("Server running on {}", server_addr);

    loop {
        let (socket, addr) = server.accept().await?;
        println!("New connection from: {}", addr);

        tokio::spawn(async move {
            let mut stream = socket;
            loop {
                let mut buffer = [0u8; 4096];
                match stream.read(&mut buffer).await {
                    anyhow::Result::Ok(0) => {
                        println!("断开连接：{}", addr);
                        return;
                    }
                    _ => (),
                }
            }
        })
        .await?;
    }
    Ok(())
}
