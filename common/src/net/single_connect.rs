use anyhow::{Ok, Result, bail};
use std::collections::VecDeque;
use std::path::Path;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::protocol::pack_message;
use crate::protocol::{self};
use crate::protocol::{MessageType, unpack_header};
use crate::utils::calculate_file_hash;

pub struct MessageReader {
    pub stream: BufReader<OwnedReadHalf>,
    pub buffer: Vec<u8>,
    pub pending_messages: VecDeque<Vec<u8>>,
}

impl MessageReader {
    fn new(stream: OwnedReadHalf) -> Self {
        Self {
            stream: BufReader::new(stream),
            buffer: Vec::with_capacity(1024 * 64), // 64KB
            pending_messages: VecDeque::new(),
        }
    }

    /// 强制等待读取1~4KB数据，返回数据长度
    pub async fn fill_buffer(&mut self) -> Result<usize> {
        let mut temp_buf = [0u8; 4096]; //4KB
        let n = self.stream.read(&mut temp_buf).await?;
        if n > 0 {
            self.buffer.extend_from_slice(&temp_buf[..n]);
        }
        Ok(n)
    }

    pub fn try_parse_message(&mut self) -> Result<()> {
        while self.buffer.len() >= 6 {
            let total_len_bytes: [u8; 4] = self.buffer[..4].try_into()?;
            let total_len = u32::from_be_bytes(total_len_bytes) as usize;

            if self.buffer.len() < total_len {
                break;
            }

            let message = self.buffer[..total_len].to_vec();
            self.pending_messages.push_back(message);

            self.buffer.drain(..total_len);
        }
        Ok(())
    }

    /// 阻塞读取下一个消息
    pub async fn read_next_message(&mut self) -> Result<Vec<u8>> {
        loop {
            self.try_parse_message()?;

            if let Some(message) = self.pending_messages.pop_front() {
                return Ok(message);
            } else {
                if self.fill_buffer().await? == 0 {
                    bail!("Connection closed");
                }
            }
        }
    }

    pub fn try_read_message(&mut self) -> Result<Option<Vec<u8>>> {
        self.try_parse_message()?;
        Ok(self.pending_messages.pop_front())
    }

    /// 阻塞读取下一个消息并解包
    pub async fn read_and_unpack_next_message(&mut self) -> Result<(MessageType, Vec<u8>)> {
        let message = self.read_next_message().await?;

        if message.len() < 6 {
            bail!("Message too short: {} bytes", message.len());
        }

        if let Some((msg_type, payload)) = unpack_header(&message) {
            Ok((msg_type, payload.to_vec()))
        } else {
            bail!("Failed to unpack message header")
        }
    }
}

pub struct MessageSender {
    pub stream: OwnedWriteHalf,
    pub msg_queue: VecDeque<Vec<u8>>,
}

impl MessageSender {
    pub fn new(stream: OwnedWriteHalf) -> Self {
        Self {
            stream,
            msg_queue: VecDeque::new(),
        }
    }

    pub async fn send_file_by_block(&mut self, path: &Path) -> Result<()> {
        let mut file = OpenOptions::new().read(true).open(path).await?;
        let file_hash = calculate_file_hash(path).await?;
        let mut buffer = [0u8; 65535];
        let mut offset = 0;

        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            let block = protocol::BlockData {
                file_hash: file_hash.clone(),
                offset,
                length: n as u32,
                data: buffer[..n].to_vec(),
            };

            offset += n as u64;

            let message = pack_message(MessageType::BlockData, &block);
            self.send_message(&message).await?;
        }

        Ok(())
    }

    pub async fn send_message(&mut self, message: &[u8]) -> Result<()> {
        self.stream.write_all(&message).await?;
        self.stream.flush().await?;

        Ok(())
    }
}

pub struct SingleConnection {
    pub sender: MessageSender,
    pub reader: MessageReader,
}

impl SingleConnection {
    pub fn from_stream(stream: TcpStream) -> Self {
        let (reader, sender) = stream.into_split();
        Self {
            sender: MessageSender::new(sender),
            reader: MessageReader::new(reader),
        }
    }
}
