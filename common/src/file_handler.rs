use crate::{
    protocol::{BlockData, DirectoryData, FileMetaData, MessageType, pack_message},
    utils::{calculate_bytes_hash, calculate_file_hash},
};
use anyhow::{Result, bail};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, atomic::AtomicUsize},
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, SeekFrom},
    sync::mpsc,
};

static RECV_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\recv_path";
static RECV_TEMP: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\temp";
static SEND_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\send_path";

#[derive(Debug)]
pub enum FileData {
    FileMetaData(FileMetaData),
    BlockData(BlockData),
    DirectoryData(DirectoryData),
}

struct FailedBlock {
    offset: u64,
    length: u32,
}

struct FileSummary {
    total_blocks: usize,
    completed_blocks: usize,
    failed_blocks: Vec<FailedBlock>,
}

pub struct FileRecvHandler {
    pub file_hash: String,
    pub worker_tx: mpsc::Sender<FileData>,
    pub task: tokio::task::JoinHandle<FileSummary>,
}

pub struct FileSendHandler {
    pub file_meta_data: FileMetaData,
    pub blocks_data: Vec<BlockData>,
    pub task: tokio::task::JoinHandle<()>,
}

impl FileRecvHandler {
    pub async fn send(&self, file_data: FileData) -> Result<()> {
        self.worker_tx.send(file_data).await?;
        Ok(())
    }

    pub fn get_pipe(&self) -> mpsc::Sender<FileData> {
        self.worker_tx.clone()
    }

    pub fn spawn(file_hash: &str) -> Self {
        let (worker_tx, worker_rx) = mpsc::channel(100);

        let task_hash = file_hash.to_string();

        let task = tokio::spawn(async move {
            match run_file_worker(worker_rx).await {
                Err(e) => {
                    eprintln!("File worker [Hash: {}] error: {}", task_hash, e);
                    FileSummary {
                        total_blocks: 0,
                        completed_blocks: 0,
                        failed_blocks: Vec::new(),
                    }
                }
                Ok(summary) => summary,
            }
        });

        Self {
            file_hash: file_hash.to_string(),
            worker_tx,
            task,
        }
    }
}

pub struct FileHandler {
    pub rx: mpsc::Receiver<FileData>,
    pub tx: mpsc::Sender<FileData>,
    pub file_workers: HashMap<String, FileRecvHandler>,
}

impl FileHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            rx,
            tx,
            file_workers: HashMap::new(),
        }
    }
    pub fn get_pipe(&self) -> mpsc::Sender<FileData> {
        self.tx.clone()
    }
}

/// 处理所有文件、目录建立相关任务，在网络消息已被解析后
impl FileHandler {
    /// 任务开启入口
    /// 路由消息到对应的文件处理任务
    pub async fn route(&mut self) -> Result<()> {
        while let Some(file_data) = self.rx.recv().await {
            self.route_file_data(file_data).await?;
        }
        Ok(())
    }

    async fn route_file_data(&mut self, file_data: FileData) -> Result<()> {
        match file_data {
            file @ (FileData::FileMetaData(_) | FileData::BlockData(_)) => {
                self.file_task(file).await?
            }
            directory @ FileData::DirectoryData(_) => self.directory_task(directory).await?,
        }
        Ok(())
    }

    async fn directory_task(&self, director: FileData) -> Result<()> {
        todo!()
    }

    /// 本地文件写入任务
    /// 1. 接收消息（FileMetaData, BlockData)
    /// 2. 建立文件处理任务（以文件Hash为Key）
    /// 3. 将新消息连接到任务
    /// 4. 任务内部执行写入操作
    /// 5. 维护一个文件块成功状态，当所有块写入成功后，删除任务。有块写入失败，发起重试
    async fn file_task(&mut self, file_data: FileData) -> Result<()> {
        // let mut file_workers: HashMap<String, mpsc::Sender<FileData>> = HashMap::new();

        // 提取文件Hash
        let file_hash = match &file_data {
            FileData::FileMetaData(meta) => meta.hash.clone(),
            FileData::BlockData(block) => block.file_hash.clone(),
            a => bail!("Wrong FileDataType: {:?}", a),
        };

        // 若不存在该文件的处理任务，则创建
        if !self.file_workers.contains_key(&file_hash) {
            let file_worker = FileRecvHandler::spawn(&file_hash);
            self.file_workers.insert(file_hash.clone(), file_worker);
        }

        // 发送文件数据到任务
        if let Some(worker) = self.file_workers.get(&file_hash) {
            if let Err(e) = worker.send(file_data).await {
                self.file_workers.remove(&file_hash);
            }
        }
        Ok(())
    }

    /// 发送文件任务
    pub async fn send_task(
        file_sender: mpsc::Sender<Vec<u8>>,
        mut command_recv: mpsc::Receiver<String>,
    ) -> Result<()> {
        while let Some(path) = command_recv.recv().await {
            let path = PathBuf::from_str(path.as_str())?;
            let meta_data = read_meta_data(&path).await?;
            // meta_data
            file_sender
                .send(pack_message(MessageType::FileMetaData, &meta_data))
                .await?;
            send_blocks(file_sender.clone(), &path).await?;
        }
        Ok(())
    }
}

async fn send_blocks(file_sender: mpsc::Sender<Vec<u8>>, path: &PathBuf) -> Result<()> {
    let file = tokio::fs::OpenOptions::new().read(true).open(path).await?;
    let mut file_reader = BufReader::new(file);
    let mut buffer = vec![0u8; 4096];
    while let Ok(n) = file_reader.read(&mut buffer).await {
        if n == 0 {
            return Ok(());
        }
        buffer.truncate(n);
        file_sender
            .send(pack_message(MessageType::BlockData, &buffer))
            .await?;
    }

    Ok(())
}

async fn run_file_worker(mut rx: mpsc::Receiver<FileData>) -> Result<FileSummary> {
    let mut total_blocks = 0;
    let mut completed_blocks = 0;
    let mut failed_blocks = Vec::new();

    let mut block_buffer = VecDeque::new();
    let mut file = None;
    while let Some(data) = rx.recv().await {
        match data {
            FileData::FileMetaData(file_meta_data) => {
                // 创建文件
                file = Some(create_meta_file(&file_meta_data).await?);
                if let Some(ref mut file_handler) = file {
                    // 写入所有缓冲块
                    while let Some(block) = block_buffer.pop_front() {
                        match write_block(file_handler, &block).await {
                            Ok(_) => {
                                total_blocks += 1;
                                completed_blocks += 1;
                            }
                            Err(_) => {
                                total_blocks += 1;
                                failed_blocks.push(FailedBlock {
                                    offset: block.offset,
                                    length: block.length,
                                });
                            }
                        }
                    }
                    block_buffer.clear();
                    block_buffer.shrink_to_fit();
                }
            }
            FileData::BlockData(block) => {
                if let Some(ref mut file_handler) = file {
                    match write_block(file_handler, &block).await {
                        Ok(_) => {
                            total_blocks += 1;
                            completed_blocks += 1;
                        }
                        Err(_) => {
                            total_blocks += 1;
                            failed_blocks.push(FailedBlock {
                                offset: block.offset,
                                length: block.length,
                            });
                        }
                    }
                } else {
                    block_buffer.push_back(block);
                    if block_buffer.len() > 1000 {
                        bail!("Too many blocks before metadata")
                    }
                }
            }
            FileData::DirectoryData(_) => {
                bail!("Wrong FileDataType");
            }
        }
    }
    Ok(FileSummary {
        total_blocks,
        completed_blocks,
        failed_blocks,
    })
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
        offset,
        data,
        self_hash,
        ..
    } = block_data;

    if !self_hash.eq_ignore_ascii_case(&calculate_bytes_hash(data)?) {
        return Err(anyhow::anyhow!("hash mismatch"));
    }

    file.seek(SeekFrom::Start(*offset)).await?;
    file.write_all(data).await?;
    Ok(())
}
