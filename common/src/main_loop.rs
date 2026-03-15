use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{
    net::single_connect::SingleConnection,
    protocol::{BlockData, FileMetaData, MessageType, create_meta_file, write_block},
};

pub async fn daemon(stream: TcpStream) -> Result<()> {
    // let (reader, writer) = stream.into_split();
    let connection = SingleConnection::from_stream(stream);

    // task1: read message
    tokio::spawn(async move {
        let mut msg_handler = connection.reader;
        loop {
            let (msg_type, payload) = msg_handler.read_and_unpack_next_message().await?;

            match msg_type {
                MessageType::HandShake => {}
                MessageType::FileMetaData => {
                    let file_meta_data: FileMetaData =
                        bincode::deserialize(&payload).context("Deserialization failed")?;
                    create_meta_file(&file_meta_data).await?;
                }
                MessageType::BlockData => {
                    let block_data: BlockData =
                        bincode::deserialize(&payload).context("Deserialization failed")?;
                    write_block(&block_data).await?;
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    });
    // task2: send message
    Ok(())
}
