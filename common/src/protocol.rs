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

static RECV_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\recv_path";
static RECV_TEMP: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\temp";
static SEND_PATH: &str = "C:\\Users\\DKL4C\\Documents\\SimpleConnect\\send_path";

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

pub async fn unpack_message(msg_type: MessageType, data: &[u8]) -> Result<()> {
    match msg_type {
        MessageType::HandShake => {}
        MessageType::FileMetaData => {
            let file_meta_data: FileMetaData =
                bincode::deserialize(data).expect("Deserialization failed");
            create_meta_file(&file_meta_data).await?;
        }
        MessageType::BlockData => {
            let block_data: BlockData = bincode::deserialize(data).expect("Deserialization failed");
            write_block(&block_data).await?;
        }
    }

    Ok(())
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

pub async fn create_meta_file(file_meta_data: &FileMetaData) -> Result<()> {
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
    let mut info = File::create(info_path).await?;

    let info_bin = bincode::serialize(file_meta_data).expect("");
    info.write_all(&info_bin).await.expect("");

    Ok(())
}

pub async fn write_block(block_data: &BlockData) -> Result<()> {
    let BlockData {
        file_hash,
        offset,
        length,
        data,
    } = block_data;
    let hash_dict = PathBuf::from_str(RECV_TEMP)?.join(file_hash[..10].to_string());
    let temp_path = hash_dict.join(file_hash);
    let info_path = hash_dict.join(format!("{}.meta", file_hash));

    let mut file = OpenOptions::new().write(true).open(temp_path).await?;
    file.seek(SeekFrom::Start(*offset)).await?;
    file.write_all(data).await?;
    Ok(())
}
