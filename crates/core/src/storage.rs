use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use tempfile::NamedTempFile;
use thiserror::Error;

use crate::{PathExposure, SCHEMA_VERSION, ScanSnapshot};

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("保存データのI/O処理に失敗しました")]
    Io(#[from] std::io::Error),
    #[error("保存データのJSONが破損しています")]
    Json(#[from] serde_json::Error),
    #[error("未対応の保存スキーマです")]
    UnsupportedSchema,
    #[error("保存データのサイズ上限を超えています")]
    TooLarge,
    #[error("保存データの整合性が不正です")]
    InvalidData,
}

/// SQLiteなどへの置き換え時にも呼び出し元をファイルI/Oから分離する。
pub trait SnapshotRepository {
    fn load(&self) -> Result<ScanSnapshot, StorageError>;
    fn save(
        &self,
        snapshot: &ScanSnapshot,
        exposure: PathExposure,
        home: Option<&Path>,
    ) -> Result<(), StorageError>;
}

pub struct JsonRepository<'a> {
    pub path: &'a Path,
}

impl SnapshotRepository for JsonRepository<'_> {
    fn load(&self) -> Result<ScanSnapshot, StorageError> {
        let mut bytes = Vec::new();
        File::open(self.path)?
            .take(MAX_JSON_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JSON_BYTES {
            return Err(StorageError::TooLarge);
        }
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        match value.get("schema_version").and_then(|v| v.as_u64()) {
            Some(1) => {
                value["schema_version"] = SCHEMA_VERSION.into();
            }
            Some(version) if version == u64::from(SCHEMA_VERSION) => {}
            _ => return Err(StorageError::UnsupportedSchema),
        }
        let snapshot: ScanSnapshot = serde_json::from_value(value)?;
        if !snapshot.validate() {
            return Err(StorageError::InvalidData);
        }
        Ok(snapshot)
    }

    fn save(
        &self,
        snapshot: &ScanSnapshot,
        exposure: PathExposure,
        home: Option<&Path>,
    ) -> Result<(), StorageError> {
        if snapshot.schema_version != SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchema);
        }
        if !snapshot.validate() {
            return Err(StorageError::InvalidData);
        }
        let bytes = serde_json::to_vec_pretty(&snapshot.project(exposure, home))?;
        if bytes.len() as u64 > MAX_JSON_BYTES {
            return Err(StorageError::TooLarge);
        }
        write_atomic(self.path, &bytes)
    }
}

pub fn save_project_view(
    path: &Path,
    view: &crate::projects::ProjectView,
    exposure: PathExposure,
    home: Option<&Path>,
) -> Result<(), StorageError> {
    if !view.snapshot.as_ref().is_some_and(|s| s.validate()) {
        return Err(StorageError::InvalidData);
    }
    let bytes = serde_json::to_vec_pretty(&view.project(exposure, home))?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(StorageError::TooLarge);
    }
    write_atomic(path, &bytes)
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // 同じディレクトリで作成・同期してから置換。先に既存ファイルを削除しない。
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|e| StorageError::Io(e.error))?;
    Ok(())
}
