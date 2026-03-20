use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::router::CopyableMessage;

// --- 1. 定义消息协议 Trait ---

pub trait RouterMessage: Send + 'static {
    type Single: Send + Sync + 'static;
    type Copyable: Clone + Send + Sync + 'static;

    fn into_content(self) -> MessageContent<Self::Single, Self::Copyable>;
}

#[derive(Debug, Clone)]
pub enum MessageContent<S, C> {
    Single(S),    // 仅支持单向点对点，不可 Clone
    Broadcast(C), // 支持 Target::All，必须可 Clone
    Empty,
}

// --- 2. 路由目标与信封 ---

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Task(String),
    All,
}

pub struct Envelope<M: RouterMessage> {
    pub from: String,
    pub target: Target,
    pub body: MessageContent<M::Single, M::Copyable>,

    pub pipe: Option<mpsc::Sender<Envelope<M>>>,
    pub respond_to: Option<oneshot::Sender<Envelope<M>>>,
}

impl<M: RouterMessage> Envelope<M> {
    /// 尝试为广播逻辑创建副本
    fn try_clone_for_broadcast(&self) -> Option<Self> {
        match &self.body {
            MessageContent::Broadcast(data) => Some(Envelope {
                from: self.from.clone(),
                target: self.target.clone(),
                body: MessageContent::Broadcast(data.clone()),
                pipe: None,
                respond_to: None, // 广播消息不承载 oneshot 回执
            }),
            MessageContent::Empty => Some(Envelope {
                from: self.from.clone(),
                target: self.target.clone(),
                body: MessageContent::Empty,
                pipe: None,
                respond_to: None, // 广播消息不承载 oneshot 回执
            }),
            MessageContent::Single(_) => None,
        }
    }
}

// --- 3. Router 控制器 ---

pub struct RouterHandler<M: RouterMessage> {
    pub inbox: mpsc::Receiver<Envelope<M>>,
    pub registry: HashMap<String, mpsc::Sender<Envelope<M>>>,
    _outbox_bridge: mpsc::Sender<Envelope<M>>,
}

impl<M: RouterMessage> RouterHandler<M> {
    pub fn new() -> (Self, RouterInterface<M>) {
        let (tx, rx) = mpsc::channel(1000);
        let mut handler = Self {
            inbox: rx,
            registry: HashMap::new(),
            _outbox_bridge: tx,
        };
        // 默认初始化主接口
        let root_interface = handler.registry("main");
        (handler, root_interface)
    }

    pub fn registry(&mut self, name: &str) -> RouterInterface<M> {
        let (tx, rx) = mpsc::channel(1000);
        self.registry.insert(name.to_string(), tx);
        RouterInterface {
            name: name.to_string(),
            outbox: self._outbox_bridge.clone(),
            inbox: rx,
        }
    }

    pub fn run(mut self) {
        tokio::spawn(async move {
            while let Some(env) = self.inbox.recv().await {
                match &env.target {
                    Target::Task(name) => {
                        // 针对 Router 自身的管理逻辑（如动态注册）可在此扩展
                        if name == "Router" {
                            if let Some(tx) = env.pipe {
                                self.registry.insert(env.from.clone(), tx);
                            }
                        } else if let Some(tx) = self.registry.get(name) {
                            let _ = tx.send(env).await;
                        }
                    }
                    Target::All => {
                        // 强制检查：只有 Broadcast 类型才处理
                        if let MessageContent::Broadcast(_) = &env.body {
                            for tx in self.registry.values() {
                                if let Some(copy) = env.try_clone_for_broadcast() {
                                    let _ = tx.send(copy).await;
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

// --- 4. 任务交互接口 ---

pub struct RouterInterface<M: RouterMessage> {
    pub name: String,
    pub outbox: mpsc::Sender<Envelope<M>>,
    pub inbox: mpsc::Receiver<Envelope<M>>,
}

impl<M: RouterMessage> RouterInterface<M> {
    /// 动态扩展接口：创建一个新的 Interface 并自动告知 Router 注册自己
    /// 需要传入一个空的 body 以符合泛型 Envelope 的构造要求
    pub async fn extend(&self, new_name: &str) -> Result<Self> {
        let (tx, rx) = mpsc::channel(1000);

        let reg_msg = Envelope {
            from: new_name.to_string(),
            target: Target::Task("Router".to_string()),
            body: MessageContent::Empty,
            pipe: Some(tx),
            respond_to: None,
        };

        self.outbox.send(reg_msg).await?;

        Ok(Self {
            name: new_name.to_string(),
            outbox: self.outbox.clone(),
            inbox: rx,
        })
    }
    /// 发送普通消息
    pub async fn send(
        &self,
        target: Target,
        body: MessageContent<M::Single, M::Copyable>,
    ) -> Result<()> {
        let env = Envelope {
            from: self.name.clone(),
            target,
            body,
            respond_to: None,
            pipe: None,
        };
        self.outbox
            .send(env)
            .await
            .map_err(|_| anyhow::anyhow!("Router 负载已关闭"))
    }

    /// 发起请求并等待回复
    pub async fn request(
        &mut self,
        target: Target,
        body: MessageContent<M::Single, M::Copyable>,
    ) -> Result<Envelope<M>> {
        let (tx, rx) = oneshot::channel();
        let env = Envelope {
            from: self.name.clone(),
            target,
            body,
            pipe: None,
            respond_to: Some(tx),
        };

        self.outbox.send(env).await?;

        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .context("请求超时")?
            .context("回复通道已关闭")
    }

    pub async fn recv(&mut self) -> Result<Envelope<M>> {
        self.inbox.recv().await.context("接收通道已关闭")
    }
}

// --- 5. 使用示例 ---

#[derive(Debug)]
enum MyEvent {
    SystemPanic(String), // Copyable
    FileChunk(Vec<u8>),  // Single
}

// 实现消息分流逻辑
impl RouterMessage for MyEvent {
    type Single = Vec<u8>;
    type Copyable = String;

    fn into_content(self) -> MessageContent<Self::Single, Self::Copyable> {
        match self {
            MyEvent::FileChunk(data) => MessageContent::Single(data),
            MyEvent::SystemPanic(msg) => MessageContent::Broadcast(msg),
        }
    }
}

#[tokio::main]
async fn main() {
    let (mut router, main_iface) = RouterHandler::<MyEvent>::new();
    let mut worker_iface = router.registry("worker");

    router.run();

    // 1. 发送广播 (Copyable)
    main_iface
        .send(
            Target::All,
            // MessageContent::Broadcast("系统维护".to_string()),
            MyEvent::SystemPanic("No".to_string()).into_content(),
        )
        .await
        .unwrap();

    // 2. 发送单播 (Single)
    main_iface
        .send(
            Target::Task("worker".to_string()),
            MessageContent::Single(vec![1, 2, 3]),
        )
        .await
        .unwrap();

    // Worker 接收验证
    while let Ok(env) = worker_iface.recv().await {
        println!("Worker 收到来自 {} 的消息: {:?}", env.from, env.body);
    }
}
