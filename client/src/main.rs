use anyhow::Result;
use common::net::MessageHandler;
use tokio::{self, net::TcpStream, time};

#[tokio::main]
async fn main() -> Result<()> {
    let server_addr = "127.0.0.1:1234";
    let mut stream = TcpStream::connect(server_addr).await?;
    let (reader, writer) = stream.into_split();
    print!("Connected to the server {}", server_addr);

    let msg_handler = MessageHandler::new(reader);

    tokio::spawn(async move {
        tokio::time::sleep(time::Duration::from_secs(5)).await;
        // let task = async {

        // };

        // if let Err(e) = task.await {
        //     eprintln!("Runtime Error: {}", e)
        // }
        Ok::<(), anyhow::Error>(())
    });

    Ok(())
}
