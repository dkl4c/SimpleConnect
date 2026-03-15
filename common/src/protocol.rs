use crate::utils::calculate_file_hash;
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
};


#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum MessageType {
    HandShake = 0x00,
    FileMetaData = 0x01,
    BlockData = 0x02,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileMetaData {
    pub name: String,
    pub hash: String,
    pub size: u64,
    pub relative_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BlockData {
    pub file_hash: String,
    pub offset: u64,
    pub length: u32,
    pub data: Vec<u8>,
}

pub fn pack_message<T: Serialize>(msg_type: MessageType, payload: &T) -> Vec<u8> {
    // (Total Length 4Bytes) (Message Type 2Bytes) (Data Body)
    // Total length means the whole length of this message pack, includes itself
    let mut body = bincode::serialize(payload).expect("Serialization failed");
    let total_len = (4 + 2 + body.len()) as u32;

    let mut frame = Vec::with_capacity(total_len as usize);
    frame.extend_from_slice(&total_len.to_be_bytes());
    frame.extend_from_slice(&(msg_type as u16).to_be_bytes());
    frame.append(&mut body);
    frame
}

pub fn unpack_header(data: &[u8]) -> Option<(MessageType, &[u8])> {
    if data.len() < 6 {
        return None;
    }

    let type_bytes = [data[4], data[5]];
    let type_code = u16::from_be_bytes(type_bytes);

    let msg_type = match type_code {
        0x00 => MessageType::HandShake,
        0x01 => MessageType::FileMetaData,
        0x02 => MessageType::BlockData,
        _ => return None,
    };

    Some((msg_type, &data[6..]))
}

