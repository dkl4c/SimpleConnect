pub mod multi_connect;
pub mod single_connect;

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

pub const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:8080";
