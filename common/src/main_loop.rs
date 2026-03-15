use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    io::SeekFrom,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

use crate::{
    net::single_connect::SingleConnection,
    protocol::{BlockData, FileMetaData, MessageType, create_meta_file, write_block},
};

pub enum FileData {
    FileMetaData(FileMetaData),
    BlockData(BlockData),
}
pub struct FileHandler {}
impl FileHandler {
    pub async fn task(mut rx: tokio::sync::mpsc::Receiver<FileData>) -> Result<()> {
        let mut file_workers: HashMap<String, mpsc::Sender<FileData>> = HashMap::new();
        while let Some(file_data) = rx.recv().await {
            let file_hash = match &file_data {
                FileData::FileMetaData(meta) => meta.hash.clone(),
                FileData::BlockData(block) => block.file_hash.clone(),
            };
            if !file_workers.contains_key(&file_hash) {
                let (worker_tx, worker_rx) = mpsc::channel(100);
                file_workers.insert(file_hash.clone(), worker_tx);

                let task_hash = file_hash.clone();
                tokio::spawn(async move {
                    if let Err(e) = run_file_worker(worker_rx).await {
                        eprintln!("File worker [Hash: {}] error: {}", task_hash, e);
                    }
                });
            }
            if let Some(tx) = file_workers.get(&file_hash) {
                if tx.send(file_data).await.is_err() {
                    file_workers.remove(&file_hash);
                }
            }
        }
        Ok(())
    }
}
async fn run_file_worker(mut rx: mpsc::Receiver<FileData>) -> Result<()> {
    let mut block_buffer = VecDeque::new();
    let mut file = None;
    while let Some(data) = rx.recv().await {
        match data {
            FileData::FileMetaData(file_meta_data) => {
                file = Some(create_meta_file(&file_meta_data).await?);
                if let Some(ref mut file_handler) = file {
                    while let Some(block) = block_buffer.pop_front() {
                        write_block(file_handler, &block).await?;
                    }
                    block_buffer.clear();
                    block_buffer.shrink_to_fit();
                }
            }
            FileData::BlockData(block_data) => {
                if let Some(ref mut file_handler) = file {
                    write_block(file_handler, &block_data).await?
                } else {
                    block_buffer.push_back(block_data);
                    if block_buffer.len() > 1000 {
                        bail!("Too many blocks before metadata")
                    }
                }
            }
        }
    }
    Ok(())
}

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
