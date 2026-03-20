use crate::{
    file_handler::{FileData, RECV_PATH, RECV_TEMP},
    node::Node::FileHandler::FileHandleTask,
    protocol::{BlockData, DirectoryData, FileMetaData},
    routerT::{MessageContent, RouterInterface, RouterMessage, Target},
    utils::calculate_bytes_hash,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, io::SeekFrom, path::PathBuf, str::FromStr};
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    task::JoinHandle,
};

pub struct FileRecvWorker {
    pub hash: String,
    pub interface: RouterInterface<FileData>,
}
impl FileRecvWorker {
    pub fn spawn(hash: String, interface: RouterInterface<FileData>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut worker = Self { hash, interface };
            worker.run().await;
        })
    }

    async fn run(&mut self) {
        let mut file: Option<LocalFileWriter> = Option::None;
        let mut block_buffer = VecDeque::new();

        while let Ok(msg) = self.interface.recv().await {
            let data = match msg.body {
                MessageContent::Single(d) => d,
                _ => continue,
            };

            match data {
                FileData::FileMetaData(meta) => {
                    let mut writer = LocalFileWriter::create(&meta).await.expect("创建失败");
                    // 处理缓冲块...
                    while let Some(b) = block_buffer.pop_front() {
                        let _ = writer.write_block(&b).await;
                    }
                    file = Some(writer);
                }

                FileData::BlockData(block) => {
                    if let Some(ref mut writer) = file {
                        // 如果写入或校验失败，回传重试请求
                        if let Err(e) = writer.write_block(&block).await {
                            eprintln!("块写入失败: {}, 请求重发 offset {}", e, block.offset);
                            let _ = self
                                .interface
                                .send(
                                    Target::Task("FileSender".to_string()),
                                    FileData::RetryRequest(block.offset).into_content(),
                                )
                                .await;
                        }

                        // 每次写完检查是否完成
                        if writer.completed() {
                            let mut writer = file.take().unwrap();
                            writer.finish().await.expect("完成转正失败");

                            // 发送 DoneConfirm 给发送端
                            let _ = self
                                .interface
                                .send(
                                    Target::Task("FileSender".to_string()),
                                    FileData::DoneConfirm.into_content(),
                                )
                                .await;

                            // 同时通知主程序
                            let _ = self
                                .interface
                                .send(
                                    Target::Task("main".to_string()),
                                    FileData::Completed.into_content(),
                                )
                                .await;

                            break; // 优雅退出
                        }
                    } else {
                        block_buffer.push_back(block);
                    }
                }

                FileData::TransmitComplete => {
                    if let Some(ref writer) = file {
                        if !writer.completed() {
                            println!("发送端已读完，但本地尚有缺失块，等待补发...");
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
pub struct FileRecvHandler {
    pub file_hash: String,
    pub task: JoinHandle<()>,
}

mod file_writer {
    use std::path::Path;

    use crate::{protocol::FileMetaData, utils::calculate_file_hash};

    pub async fn read_meta_data(path: &Path) -> FileMetaData {
        let file_meta = path
            .metadata()
            .expect(&format!("无法获取文件元数据：{:?}", path));
        let filename = path.file_name().unwrap().to_str().unwrap().to_string();
        FileMetaData {
            name: filename,
            hash: calculate_file_hash(path).await.expect("无法解析文件hash"),
            size: file_meta.len(),
            relative_path: "".to_string(),
        }
    }
}
pub struct FileProgress {}
#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInfo {
    file_hash: String,
    offset: u64,
    length: u32,
}
pub struct LocalFileWriter {
    pub file: tokio::fs::File,
    pub meta_file: tokio::fs::File,
    pub meta_data: FileMetaData,

    pub total_block_cnt: u64,
    pub complete_block_cnt: u64,
    pub failed_blocks: Vec<BlockInfo>,
    pub complete_blocks: Vec<BlockInfo>,

    pub complete_bytes: u64,
    pub uncommited_cnt: u64,
}
impl LocalFileWriter {
    pub async fn create(file_meta_data: &FileMetaData) -> Result<Self> {
        let FileMetaData { hash, size, .. } = file_meta_data;

        let hash_dict = PathBuf::from_str(RECV_TEMP)?.join(hash[..10].to_string());
        let temp_path = hash_dict.join(hash);
        let info_path = hash_dict.join(format!("{}.meta", hash));

        tokio::fs::create_dir_all(hash_dict).await?;

        let file = File::create(temp_path).await?;
        file.set_len(*size).await?;

        let meta_file = File::create(info_path).await?;

        Ok(Self {
            file,
            meta_file,
            meta_data: file_meta_data.clone(),
            total_block_cnt: 0,
            complete_block_cnt: 0,
            failed_blocks: Vec::new(),
            complete_blocks: Vec::new(),
            complete_bytes: 0,
            uncommited_cnt: 0,
        })
    }

    pub async fn write_block(&mut self, block_data: &BlockData) -> Result<()> {
        let BlockData {
            offset,
            data,
            self_hash,
            file_hash,
            length,
        } = block_data;

        let block_info = BlockInfo {
            file_hash: file_hash.clone(),
            offset: *offset,
            length: *length,
        };

        let write: Result<()> = {
            if !file_hash.eq_ignore_ascii_case(&self.meta_data.hash) {
                panic!("文件传输系统出错，块哈希与文件哈希不匹配")
            }

            if !self_hash.eq_ignore_ascii_case(&calculate_bytes_hash(data)?) {
                return Err(anyhow::anyhow!("hash mismatch"));
            }

            self.file.seek(SeekFrom::Start(*offset)).await?;
            self.file.write_all(data).await?;
            Ok(())
        };

        if let Err(_) = write {
            self.failed_blocks.push(block_info);
            return Err(anyhow::anyhow!("write block failed"));
        }

        self.total_block_cnt += 1;
        self.complete_block_cnt += 1;
        self.complete_bytes += *length as u64;
        self.complete_blocks.push(block_info);
        self.uncommited_cnt += 1;

        self.flush_meta_file().await?;

        Ok(())
    }

    pub async fn flush_meta_file(&mut self) -> Result<()> {
        if self.uncommited_cnt < 5 && !self.completed() {
            return Ok(());
        }

        let info = bincode::serialize(&self.complete_blocks).unwrap();

        self.meta_file.rewind().await?;
        self.meta_file.write_all(&info).await?;
        self.meta_file.set_len(info.len() as u64).await?;
        self.meta_file.sync_all().await?;
        self.uncommited_cnt = 0;

        Ok(())
    }

    pub async fn finish(&mut self) -> Result<()> {
        // 1. 最后强制刷一次盘（确保所有数据落盘）
        self.file.sync_all().await?;
        self.meta_file.sync_all().await?;

        // 2. 构造路径
        let final_path = PathBuf::from_str(RECV_PATH)?.join(&self.meta_data.name);
        let temp_path = PathBuf::from_str(RECV_TEMP)?
            .join(&self.meta_data.hash[..10])
            .join(&self.meta_data.hash);
        let meta_path = PathBuf::from_str(RECV_TEMP)?
            .join(&self.meta_data.hash[..10])
            .join(format!("{}.meta", self.meta_data.hash));

        // 3. 移动文件并删除 meta
        tokio::fs::rename(temp_path, final_path).await?;
        let _ = tokio::fs::remove_file(meta_path).await;

        Ok(())
    }

    pub fn completed(&self) -> bool {
        self.complete_bytes == self.meta_data.size
    }

    pub fn failed(&self) -> bool {
        !self.failed_blocks.is_empty()
    }
}

pub async fn handle_directory(directory_data: &DirectoryData) {
    let mut path = PathBuf::from_str(RECV_PATH).expect(&format!("无法解析存储目录: {}", RECV_PATH));
    let relative_path =
        PathBuf::from_str(&directory_data.path).expect(&format!("无法解析目录: {}", RECV_PATH));

    path.push(relative_path);

    tokio::fs::create_dir_all(&path).await.expect(&format!(
        "创建目录失败: {}",
        path.to_str().expect("无法解析目录")
    ));
}
