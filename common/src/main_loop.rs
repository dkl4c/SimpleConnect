use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::net::MessageHandler;

pub async fn main_loop(stream: TcpStream) -> Result<()> {
    let msg_handler = MessageHandler::new(stream);

    Ok(())
}
