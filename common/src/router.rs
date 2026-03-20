use crate::{
    file_handler::FileData,
    routerT::{MessageContent, RouterMessage},
};
#[derive(Debug, Clone)]

pub enum MessageType {
    File,
    Directory,
    Ctrl,
    Message,
    Registry,
}
#[derive(Clone)]
pub enum CopyableMessage {
    String(String),
    Heartbeat,
}

pub enum Message {
    Message(CopyableMessage),
    FileData(FileData),
}

impl RouterMessage for Message {
    fn into_content(self) -> crate::routerT::MessageContent<Self::Single, Self::Copyable> {
        match self {
            Message::Message(msg) => MessageContent::Broadcast(msg),
            data @ Message::FileData(_) => MessageContent::Single(data),
        }
    }

    type Single = Message;

    type Copyable = CopyableMessage;
}
