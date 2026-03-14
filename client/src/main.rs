use anyhow::Result;
use tokio::{self, net::TcpStream};

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr = "127.0.0.1:1234";
    let mut stream = TcpStream::connect(server_addr).await?;
    print!("Connected to the server {}", server_addr);
    
    
    Ok(())
}
