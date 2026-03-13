pub mod net;
pub mod protocol;
pub mod utils;

#[cfg(test)]
mod utils_test {
    use std::{path::PathBuf, str::FromStr};

    use crate::{
        protocol::{create_meta_file, read_meta_data},
        utils::normalize_path,
    };

    #[tokio::test]
    pub async fn hash() {
        use std::{path::PathBuf, str::FromStr};

        use crate::utils;

        let hash = utils::calculate_file_hash(
            &PathBuf::from_str(
                normalize_path("C:\\Users\\DKL4C\\Documents\\SimpleConnect\\CalculateHashTest.txt")
                    .as_str(),
            )
            .unwrap(),
        )
        .await
        .unwrap();

        println!("{}", hash);
        assert_eq!(
            hash,
            "ce271f3b5a1f1aa33a1775c7dd80186d0c8b798278c491bc6ca1c55ab12ecc01".to_string()
        )
    }

    #[tokio::test]
    pub async fn t() {
        let path = PathBuf::from_str(
            normalize_path("C:\\Users\\DKL4C\\Documents\\SimpleConnect\\CalculateHashTest.txt")
                .as_str(),
        )
        .unwrap();
        let file_meta = read_meta_data(&path).await.unwrap();
        let _ = create_meta_file(&file_meta).await;
    }
}
