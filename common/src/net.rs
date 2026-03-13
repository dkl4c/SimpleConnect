use anyhow::Result;
use std::path::Path;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::protocol;
use crate::protocol::MessageType;
use crate::protocol::pack_message;
use crate::utils::calculate_file_hash;

const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:8080";

pub async fn send_file_by_block(stream: &mut TcpStream, path: &Path) -> Result<()> {
    let mut file = OpenOptions::new().read(true).open(path).await?;
    let file_hash = calculate_file_hash(path).await?;
    let mut buffer = [0u8; 65535];
    let mut offset = 0;

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }

        let block = protocol::BlockData {
            file_hash: file_hash.clone(),
            offset,
            length: n as u32,
            data: buffer[..n].to_vec(),
        };

        offset += n as u64;

        let message = pack_message(MessageType::BlockData, &block);
        send_message(stream, &message).await?;
    }

    Ok(())
}

pub async fn send_message(stream: &mut TcpStream, message: &[u8]) -> Result<()> {
    stream.write_all(message).await?;
    stream.flush().await?;

    Ok(())
}

pub async fn read_message(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let len = stream.read_u32().await? as usize;
    let mut buffer = vec![0u8; len];
    stream.read_exact(&mut buffer).await?;

    Ok(buffer)
}
