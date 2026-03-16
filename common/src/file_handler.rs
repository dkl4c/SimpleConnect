use crate::{
    protocol::{BlockData, FileMetaData},
    utils::calculate_file_hash,
};
use anyhow::{Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::mpsc,
};

static RECV_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\recv_path";
static RECV_TEMP: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\temp";
static SEND_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\send_path";

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

pub async fn read_meta_data(path: &Path) -> Result<FileMetaData> {
    let file_meta = path.metadata()?;
    let filename = path.file_name().unwrap().to_str().unwrap().to_string();
    Ok(FileMetaData {
        name: filename,
        hash: calculate_file_hash(path).await?,
        size: file_meta.len(),
        relative_path: "".to_string(),
    })
}

pub async fn create_meta_file(file_meta_data: &FileMetaData) -> Result<File> {
    let FileMetaData {
        name,
        hash,
        size,
        relative_path,
    } = file_meta_data;

    let hash_dict = PathBuf::from_str(RECV_TEMP)?.join(hash[..10].to_string());
    let temp_path = hash_dict.join(hash);
    let info_path = hash_dict.join(format!("{}.meta", hash));

    let mut dict = tokio::fs::create_dir_all(hash_dict).await?;
    let mut file = File::create(temp_path).await?;
    let mut info = File::create(info_path).await?;

    let info_bin = bincode::serialize(file_meta_data).expect("");
    info.write_all(&info_bin).await.expect("");

    Ok(file)
}

pub async fn write_block(file: &mut File, block_data: &BlockData) -> Result<()> {
    let BlockData {
        file_hash,
        offset,
        length,
        data,
    } = block_data;
    // let hash_dict = PathBuf::from_str(RECV_TEMP)?.join(file_hash[..10].to_string());
    // let temp_path = hash_dict.join(file_hash);
    // let info_path = hash_dict.join(format!("{}.meta", file_hash));

    // let mut file = OpenOptions::new().write(true).open(temp_path).await?;
    file.seek(SeekFrom::Start(*offset)).await?;
    file.write_all(data).await?;
    Ok(())
}
