use anyhow::{Ok, Result};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr = "127.0.0.1:1234";
    let server = TcpListener::bind(server_addr).await?;
    println!("Server running on {}", server_addr);

    loop {
        let (mut socket, addr) = server.accept().await?;
        println!("New connection from: {}", addr);

        tokio::spawn(async move {});
    }
    Ok(())
}
