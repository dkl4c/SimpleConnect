use anyhow::{Ok, Result};
use common::{net::single_connect::SingleConnection, protocol::MessageType};
use tokio::{io::AsyncReadExt, net::TcpListener};

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr = "127.0.0.1:1234";
    let server = TcpListener::bind(server_addr).await?;
    println!("Server running on {}", server_addr);

    loop {
        let (stream, addr) = server.accept().await?;
        println!("New connection from: {}", addr);

        let mut connection = SingleConnection::from_stream(stream);

        tokio::spawn(async move {
            loop {
                match connection.reader.read_and_unpack_next_message().await {
                    anyhow::Result::Ok((MessageType::CloseConnection, _))
                    | anyhow::Result::Err(_) => {
                        println!("断开连接：{}", addr);
                        return;
                    }
                    anyhow::Result::Ok((a, _)) => {
                        println!("受到数据包类型：{:?}", a);
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
