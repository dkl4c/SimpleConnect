use crate::{
    protocol::{BlockData, DirectoryData, FileMetaData},
    router::Message,
    routerT::{Envelope, MessageContent, RouterHandler, RouterInterface, RouterMessage, Target},
    utils::{calculate_bytes_hash, calculate_file_hash},
};
use anyhow::Result;
use std::{
    collections::{HashMap, VecDeque},
    io::SeekFrom,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    task::JoinHandle,
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

impl RouterMessage for FileData {
    type Single = FileData;

    type Copyable = String;

    fn into_content(self) -> crate::routerT::MessageContent<Self::Single, Self::Copyable> {
        todo!()
    }
}

pub struct FileHandler {
    pub router: RouterInterface<Message>,
    pub file_senders: HashMap<String, FileSendHandler>,
    pub file_recvers: HashMap<String, FileRecvHandler>,

    pub local_router: Option<RouterHandler<FileData>>,
    pub local_router_interface: RouterInterface<FileData>,
}

impl FileHandler {
    pub fn new(router: RouterInterface<Message>) -> Self {
        let (local_router, local_router_interface) = RouterHandler::new();
        Self {
            router,
            file_senders: HashMap::new(),
            file_recvers: HashMap::new(),
            local_router: Some(local_router),
            local_router_interface,
        }
    }
    pub fn run(mut self) {
        let router = self
            .local_router
            .take()
            .expect("无法启动内部router：所有权不存在");
        router.run();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(ext_env) = self.router.recv() => {
                        match &ext_env.target {
                            crate::routerT::Target::Task(name) => {
                                if name != "FileHandler" {
                                    continue;
                                }
                            }
                            _ => {}
                        };
                        self.handle_external_dispatch(ext_env).await;
                    },

                    Ok(loc_env) = self.local_router_interface.recv() => {
                        self.handle_internal_feedback(loc_env).await;
                    }
                }
            }
        });
    }
    async fn handle_external_dispatch(&mut self, env: Envelope<Message>) {
        let file_data = if let MessageContent::Single(Message::FileData(data)) = &env.body {
            data
        } else {
            return;
        };

        let hash = match file_data {
            FileData::FileMetaData(d) => d.hash.clone(),
            FileData::BlockData(d) => d.file_hash.clone(),
            FileData::DirectoryData(d) => {
                handle_directory(d).await;
                return;
            }
            _ => return,
        };

        if !self.file_recvers.contains_key(&hash) {
            let worker_interface = self
                .local_router_interface
                .extend(&hash)
                .await
                .expect("注册任务消息接口失败");
            let task = FileRecvWorker::spawn(hash.clone(), worker_interface);
            self.file_recvers.insert(
                hash.clone(),
                FileRecvHandler {
                    file_hash: hash.clone(),
                    task,
                },
            );
        }
        self.local_router_interface
            .send(
                Target::Task(hash),
                try_convert_into_file_data(env.body).expect("消息不是FileData，无法转换"),
            )
            .await
            .expect("发送消息失败");
    }
    async fn handle_internal_feedback(&mut self, msg: Envelope<FileData>) {}
}

fn try_convert_into_file_data(
    body: MessageContent<Message, crate::router::CopyableMessage>,
) -> Option<MessageContent<FileData, String>> {
    if let MessageContent::Single(Message::FileData(data)) = body {
        Some(MessageContent::Single(data))
    } else {
        None
    }
}
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
        let mut total_blocks = 0;
        let mut completed_blocks = 0;
        let mut failed_blocks = Vec::new();

        let mut block_buffer = VecDeque::new();
        let mut file = None;
        while let Ok(data) = self.interface.recv().await {
            let data = match data.body {
                MessageContent::Single(data) => data,
                _ => continue,
            };
            match data {
                FileData::FileMetaData(file_meta_data) => {
                    // 创建文件
                    file = Some(
                        create_file_by_meta(&file_meta_data)
                            .await
                            .expect("写入失败"),
                    );
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
                            panic!("Too many blocks before metadata")
                        }
                    }
                }
                FileData::DirectoryData(_) => {
                    panic!("Wrong FileDataType");
                }
            }
        }
    }
}
pub struct FileRecvHandler {
    pub file_hash: String,
    pub task: JoinHandle<()>,
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

pub async fn create_file_by_meta(file_meta_data: &FileMetaData) -> Result<File> {
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
    file.set_len(size.clone()).await;
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
async fn handle_directory(directory_data: &DirectoryData) {
    let mut path = PathBuf::from_str(RECV_PATH).expect(&format!("无法解析存储目录: {}", RECV_PATH));
    let relative_path =
        PathBuf::from_str(&directory_data.path).expect(&format!("无法解析目录: {}", RECV_PATH));

    path.push(relative_path);

    tokio::fs::create_dir_all(&path).await.expect(&format!(
        "创建目录失败: {}",
        path.to_str().expect("无法解析目录")
    ));
}

pub struct FileSendHandler {}

pub struct FailedBlock {
    offset: u64,
    length: u32,
}
