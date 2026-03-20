use anyhow::Result;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, BufReader},
    sync::mpsc,
    task::JoinHandle,
};

use crate::{
    file_handler::{FileData, read_meta_data},
    protocol::{BlockData, FileMetaData, MessageType, pack_message},
    routerT::{MessageContent, RouterInterface, RouterMessage, Target},
    utils::{calculate_bytes_hash, calculate_file_hash},
};

pub struct FileSendWorker {
    pub path: PathBuf,
    pub file_meta: FileMetaData,
    pub interface: RouterInterface<FileData>,
}

impl FileSendWorker {
    pub fn spawn(
        path: PathBuf,
        file_meta: FileMetaData,
        interface: RouterInterface<FileData>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut worker = Self {
                path,
                file_meta,
                interface,
            };
            if let Err(e) = worker.run().await {
                eprintln!("发送任务 {} 出错: {}", worker.file_meta.name, e);
            }
        })
    }

    async fn run(&mut self) -> Result<()> {
        let file = tokio::fs::File::open(&self.path).await?;
        let mut reader = tokio::io::BufReader::new(file);
        let mut offset = 0u64;
        let chunk_size = 64 * 1024;
        let mut buffer = vec![0u8; chunk_size];

        // 1. 发送元数据
        self.interface
            .send(
                Target::Task("FileHandler".to_string()),
                FileData::FileMetaData(self.file_meta.clone()).into_content(),
            )
            .await?;

        let mut transmit_done = false;

        loop {
            tokio::select! {
                // 监听接收端反馈 (重试请求或完成确认)
                Ok(msg) = self.interface.recv() => {
                    if let MessageContent::Single(data) = msg.body {
                        match data {
                            FileData::RetryRequest(off) => {
                                println!("收到重试请求: offset {}", off);
                                self.retransmit_block(off).await?;
                            }
                            FileData::DoneConfirm => {
                                println!("接收端确认完成，发送任务成功结束");
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }

                // 只有在还没读完文件时，才执行读取逻辑
                n = reader.read(&mut buffer), if !transmit_done => {
                    let n = n?;
                    if n == 0 {
                        transmit_done = true;
                        // 告知接收端：我这边已经读完了
                        self.interface.send(
                            Target::Task("FileHandler".to_string()),
                            FileData::TransmitComplete.into_content()
                        ).await?;
                        continue;
                    }

                    let chunk_data = buffer[..n].to_vec();
                    let block = BlockData {
                        file_hash: self.file_meta.hash.clone(),
                        offset,
                        length: n as u32,
                        self_hash: crate::utils::calculate_bytes_hash(&chunk_data)?,
                        data: chunk_data,
                    };

                    self.interface.send(
                        Target::Task("FileHandler".to_string()),
                        FileData::BlockData(block).into_content()
                    ).await?;

                    offset += n as u64;
                }

                // 超时处理：如果读完了之后 30 秒还没收到 DoneConfirm，强制退出
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)), if transmit_done => {
                    return Err(anyhow::anyhow!("等待接收端确认超时"));
                }
            }
        }
    }
    // 定点重传函数
    async fn retransmit_block(&mut self, off: u64) -> Result<()> {
        use std::io::SeekFrom;
        let mut file = tokio::fs::File::open(&self.path).await?;
        file.seek(SeekFrom::Start(off)).await?;

        let mut buf = vec![0u8; 64 * 1024];
        let n = file.read(&mut buf).await?;
        buf.truncate(n);

        let block = BlockData {
            file_hash: self.file_meta.hash.clone(),
            offset: off,
            length: n as u32,
            self_hash: crate::utils::calculate_bytes_hash(&buf)?,
            data: buf,
        };
        self.interface
            .send(
                Target::Task("FileHandler".to_string()),
                FileData::BlockData(block).into_content(),
            )
            .await?;
        Ok(())
    }
}
pub struct FileSendHandler {
    pub file_tasks: HashMap<String, JoinHandle<()>>,
}

impl FileSendHandler {
    pub fn new() -> Self {
        Self {
            file_tasks: HashMap::new(),
        }
    }

    /// 开启一个新的发送任务
    pub async fn start_send_task(
        &mut self,
        path: PathBuf,
        local_router_interface: &RouterInterface<FileData>,
    ) -> Result<()> {
        // 1. 生成元数据
        let meta = read_meta_data(&path).await?;
        let hash = meta.hash.clone();

        // 2. 扩展内部路由接口给该 Worker
        let worker_interface = local_router_interface.extend(&hash).await?;

        // 3. 启动 Worker
        let handle = FileSendWorker::spawn(path, meta, worker_interface);

        self.file_tasks.insert(hash, handle);
        Ok(())
    }
}
