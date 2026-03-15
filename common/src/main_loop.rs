use anyhow::Result;
use fs2::FileExt;
use std::fs::File;
use tokio::{net::TcpStream, time::Duration};

use crate::net::single_connect::SingleConnection;

pub async fn daemon(server_addr: &str) -> Result<()> {
    let daemon_lock = File::create("/tmp/SimpleConnect.deamon.lock").unwrap();
    println!("守护进程已启动inner");
    if daemon_lock.try_lock_exclusive().is_err() {
        println!("另一个守护进程正在运行中。");
        return Ok(());
    }
    let stream = TcpStream::connect(server_addr).await?;
    let connection = SingleConnection::from_stream(stream);

    // task1: read message
    tokio::spawn(async move {
        // let mut msg_handler = connection.reader;
        // loop {
        //     let (msg_type, payload) = msg_handler.read_and_unpack_next_message().await?;

        //     match msg_type {
        //         MessageType::HandShake => {}
        //         MessageType::FileMetaData => {}
        //         MessageType::BlockData => {}
        //     }
        // }

        tokio::time::sleep(Duration::from_secs(3)).await;
        Ok::<(), anyhow::Error>(())
    })
    .await?;
    // task2: send message
    println!("exited daemon");
    Ok(())
}
