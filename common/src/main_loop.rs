use anyhow::Result;
use fs2::FileExt;
use std::{io::Write, process, time::Duration};
use tokio::net::TcpStream;

use crate::file_handler::{FileData, FileHandler};
use crate::protocol::{BlockData, FileMetaData, NetworkMessage};
use crate::{
    net::single_connect::SingleConnection,
    protocol::{Heartbeat, MessageType, pack_message},
};

pub static DAEMON_LOCK: &str = "/tmp/SimpleConnect.deamon.lock";

pub async fn daemon(server_addr: &str, args: &[String]) -> Result<()> {
    let mut daemon_lock = std::fs::File::create(DAEMON_LOCK).unwrap();
    let pid = process::id();
    if daemon_lock.try_lock_exclusive().is_err() {
        eprintln!("另一个守护进程正在运行中，退出当前进程: {}", pid);
        return Ok(());
    }

    println!("守护进程已启动，PID: {}", pid);
    daemon_lock.set_len(0)?;
    daemon_lock.write_all(pid.to_string().as_bytes())?;
    daemon_lock.flush()?;

    let stream = TcpStream::connect(server_addr).await?;
    let mut connection = SingleConnection::from_stream(stream);

    let (msg_send_center_sender, mut msg_send_center) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let (file_pipe, file_receiver) = tokio::sync::mpsc::channel::<FileData>(100);

    // task: File Handler
    tokio::spawn(async move {
        if let Err(e) = FileHandler::task(file_receiver).await {
            eprintln!("FileHandler task error: {}", e);
        }
    });
    // task: Message Send Center
    tokio::spawn(async move {
        while let Some(ref msg) = msg_send_center.recv().await {
            connection.sender.send_message(msg).await?;
        }
        Ok::<(), anyhow::Error>(())
    });

    // task: Heartbeat
    let heartbeat_sender = msg_send_center_sender.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            heartbeat_sender
                .send(pack_message(MessageType::Heartbeat, &Heartbeat))
                .await?;
        }
        Ok::<(), anyhow::Error>(())
    });

    // task: Send File
    let file_sender = msg_send_center_sender.clone();
    tokio::spawn(async move {
        loop {
            file_sender
                .send(pack_message(
                    MessageType::FileMetaData,
                    &FileMetaData {
                        name: todo!(),
                        hash: todo!(),
                        size: todo!(),
                        relative_path: todo!(),
                    },
                ))
                .await?;
        }
        Ok::<(), anyhow::Error>(())
    });

    // task: read message
    let file_handler = file_pipe.clone();
    tokio::spawn(async move {
        loop {
            let (msg_type, payload) = connection.reader.read_and_unpack_next_message().await?;

            match msg_type {
                MessageType::Heartbeat => {}
                MessageType::FileMetaData => {
                    let file_meta_data = FileMetaData::from_bytes(&payload)?;
                    file_handler
                        .send(FileData::FileMetaData(file_meta_data))
                        .await?;
                }
                MessageType::BlockData => {
                    let block_data = BlockData::from_bytes(&payload)?;
                    file_handler.send(FileData::BlockData(block_data)).await?;
                }
                MessageType::CloseConnection => {
                    break;
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    });
    tokio::signal::ctrl_c().await?;
    println!("exited daemon");
    Ok(())
}
