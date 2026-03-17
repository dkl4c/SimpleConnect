use std::time::Duration;
use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use anyhow::Result;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub enum Target {
    Task(String),
    All,
}
impl ToString for Target {
    fn to_string(&self) -> String {
        match self {
            Target::Task(name) => name.clone(),
            Target::All => "all".to_string(),
        }
    }
}
#[derive(Debug, Clone)]
pub enum MessageType {
    File(),
    Directory(),
    Ctrl(),
    Message(),
}
#[derive(Debug)]
pub struct Message {
    from: String,
    target: Target,
    msg_type: MessageType,
    body: MessageBody,

    respond_to: Option<oneshot::Sender<Message>>,
}

#[derive(Debug, Clone)]
pub enum MessageBody {
    String(String),
}

pub struct RouterHandler {
    pub inbox: tokio::sync::mpsc::Receiver<Message>,
    pub registry: HashMap<String, tokio::sync::mpsc::Sender<Message>>,
    _inbox_front: tokio::sync::mpsc::Sender<Message>,
}
impl RouterHandler {
    pub fn run(mut self) {
        tokio::spawn(async move {
            while let Some(msg) = self.inbox.recv().await {
                match msg.target.clone() {
                    Target::Task(target_name) => {
                        if let Some(target_tx) = self.registry.get(&target_name) {
                            if let Err(_) = target_tx.send(msg).await {
                                self.registry.remove(&target_name);
                            }
                        }
                    }
                    Target::All => {
                        let mut dead = Vec::new();
                        for (name, tx) in self.registry.iter() {
                            if let Err(_) = tx.send(msg.copy_broadcast()).await {
                                dead.push(name.clone());
                            }
                        }
                        for name in dead {
                            self.registry.remove(&name);
                        }
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        });
    }
}
impl RouterHandler {
    pub fn new() -> Self {
        let (sender_net, receiver_here) = tokio::sync::mpsc::channel(1000);

        RouterHandler {
            inbox: receiver_here,
            _inbox_front: sender_net,
            registry: HashMap::new(),
        }
    }
    pub fn registry(&mut self, task_name: String) -> RouterInterface {
        let (sender, recv) = tokio::sync::mpsc::channel(1000);
        self.registry.insert(task_name.clone(), sender);
        RouterInterface {
            name: task_name,
            outbox: self._inbox_front.clone(),
            inbox: recv,
        }
    }
}
pub struct RouterInterface {
    pub name: String,
    pub outbox: tokio::sync::mpsc::Sender<Message>,
    pub inbox: tokio::sync::mpsc::Receiver<Message>,
}
impl RouterInterface {
    pub async fn request(
        &mut self,
        target: Target,
        msg_type: MessageType,
        body: MessageBody,
    ) -> Result<Message> {
        let (tx, rx) = oneshot::channel();

        let msg = Message {
            from: self.name.clone(),
            target: target,
            msg_type,
            body,

            respond_to: Some(tx),
        };

        self.send(msg).await?;

        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .context("请求超时")?
            .context("获取反馈失败")
    }
    pub async fn send(&mut self, msg: Message) -> Result<()> {
        self.outbox.send(msg).await?;
        Ok(())
    }
    pub async fn recv(&mut self) -> Result<Message> {
        self.inbox.recv().await.context("")
    }
}
impl Message {
    pub fn reply(self, msg_type: MessageType, body: MessageBody) -> Result<()> {
        if let Some(tx) = self.respond_to {
            let response = Message {
                from: self.target.to_string(),
                target: Target::Task(self.from),
                msg_type,
                body,
                respond_to: None,
            };
            tx.send(response).map_err(|_| anyhow::anyhow!("回复失败"))?;
        }
        Ok(())
    }

    fn copy_broadcast(&self) -> Message {
        Message {
            respond_to: None,
            from: self.from.clone(),
            target: self.target.clone(),
            msg_type: self.msg_type.clone(),
            body: self.body.clone(),
        }
    }
}

fn main() {
    let router = RouterHandler::new();
    router.run();
    router.registry(task_name);
}
