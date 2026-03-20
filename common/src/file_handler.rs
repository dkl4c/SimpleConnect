use std::{collections::HashMap, path::PathBuf, str::FromStr};

use tokio::{fs::File, task::JoinHandle};

use crate::{
    protocol::{BlockData, DirectoryData, FileMetaData},
    router::Message,
    routerT::{Envelope, MessageContent, RouterHandler, RouterInterface, RouterMessage},
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
            while let Ok(msg) = self.router.recv().await {
                match &msg.target {
                    crate::routerT::Target::Task(name) => {
                        if name != "FileHandler" {
                            continue;
                        }
                    }
                    _ => {}
                };

                // Msg for me
                self.handle_message(msg).await;
            }
        });
    }
}
impl FileHandler {
    async fn handle_message(&mut self, msg: Envelope<Message>) {
        if let MessageContent::Single(data) = msg.body {
            match data {
                Message::Message(plain_msg) => todo!(),
                Message::FileData(file_data) => self.handle_file_data(file_data).await,
            }
        }
    }
    async fn handle_file_data(&mut self, file_data: FileData) {
        match file_data {
            FileData::FileMetaData(_) | FileData::BlockData(_) => {
                self.handle_single_file_data(file_data).await
            }
            FileData::DirectoryData(directory_data) => self.handle_directory(directory_data).await,
        }
    }
    async fn handle_single_file_data(&mut self, file_data: FileData) {
        let file_hash = match &file_data {
            FileData::FileMetaData(file_meta_data) => file_meta_data.hash.clone(),
            FileData::BlockData(block_data) => block_data.file_hash.clone(),
            _ => {
                panic!("错误的文件数据类型")
            }
        };

        if let Some(worker) = self.file_recvers.get(&file_hash) {
        } else {
            let file_recver_router_interface = self
                .local_router_interface
                .extend(&file_hash)
                .await
                .expect("注册消息接口失败");
            let file_recver = FileRecvHandler::spawn(&file_hash, file_recver_router_interface);
            self.file_recvers.insert(file_hash.clone(), file_recver);
        }
    }
    async fn handle_directory(&mut self, directory_data: DirectoryData) {
        let mut path =
            PathBuf::from_str(RECV_PATH).expect(&format!("无法解析存储目录: {}", RECV_PATH));
        let relative_path =
            PathBuf::from_str(&directory_data.path).expect(&format!("无法解析目录: {}", RECV_PATH));

        path.push(relative_path);

        tokio::fs::create_dir_all(&path).await.expect(&format!(
            "创建目录失败: {}",
            path.to_str().expect("无法解析目录")
        ));
    }
}

pub struct FileRecvHandler {
    pub file_hash: String,
    // pub router_interface: RouterInterface<FileData>,
    pub task: JoinHandle<()>,
}
impl FileRecvHandler {
    pub fn spawn(file_hash: &str, router_interface: RouterInterface<FileData>) -> Self {
        let task = tokio::spawn(async move {
            Self::run_file_recver(router_interface).await;
        });
        Self {
            file_hash: file_hash.to_string(),
            // router_interface,
            task,
        }
    }

    async fn run_file_recver(router_interface: RouterInterface<FileData>) -> () {
        todo!()
    }
}
pub struct FileSendHandler {}
