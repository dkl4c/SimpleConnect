mod recver;
mod sender;

use crate::{
    file_handler::{
        recver::{FileRecvHandler, FileRecvWorker, handle_directory},
        sender::FileSendHandler,
    },
    protocol::{BlockData, DirectoryData, FileMetaData},
    router::Message,
    routerT::{Envelope, MessageContent, RouterHandler, RouterInterface, RouterMessage, Target},
    utils::calculate_file_hash,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

static RECV_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\recv_path";
static RECV_TEMP: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\temp";
static SEND_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\send_path";

// #[derive(Debug)]
// pub enum FileData {
//     FileMetaData(FileMetaData),
//     BlockData(BlockData),
//     DirectoryData(DirectoryData),
//     RequestRetry(String, u64, u64),
//     Completed,
// }
#[derive(Debug, Serialize, Deserialize)]
pub enum FileData {
    FileMetaData(FileMetaData),
    BlockData(BlockData),
    DirectoryData(DirectoryData),

    Completed,
    // 接收端 -> 发送端：请求重发特定块
    RetryRequest(u64),
    // 发送端 -> 接收端：磁盘读取完毕
    TransmitComplete,
    // 接收端 -> 发送端：所有块写完且校验通过，任务成功
    DoneConfirm,
}
impl RouterMessage for FileData {
    type Single = FileData;
    type Copyable = String;
    fn into_content(self) -> crate::routerT::MessageContent<Self::Single, Self::Copyable> {
        match self {
            // FileData::FileMetaData(file_meta_data) => {
            //     MessageContent::Single(FileData::FileMetaData(file_meta_data))
            // }
            // FileData::BlockData(block_data) => {
            //     MessageContent::Single(FileData::BlockData(block_data))
            // }
            // FileData::DirectoryData(directory_data) => {
            //     MessageContent::Single(FileData::DirectoryData(directory_data))
            // }
            // FileData::Completed => MessageContent::Single(FileData::Completed),
            // FileData::RetryRequest(_) => todo!(),
            // FileData::TransmitComplete => todo!(),
            // FileData::DoneConfirm => todo!(),
            msg @ _ => MessageContent::Single(msg),
        }
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
        Some(data.into_content())
        // Some(MessageContent::Single(data))
    } else {
        None
    }
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
