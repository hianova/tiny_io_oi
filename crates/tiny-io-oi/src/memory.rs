extern crate alloc;

pub use crate::unsafe_core::{TinyArc, Arena};

use spin::Mutex;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

pub struct FlashFileSystem {
    pub storage: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl FlashFileSystem {
    pub fn new() -> Self {
        Self {
            storage: Mutex::new(BTreeMap::new()),
        }
    }
}

impl cdDB::FileSystem for FlashFileSystem {
    fn write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        let mut store = self.storage.lock();
        store.insert(path.to_string(), data.to_vec());
        Ok(())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let store = self.storage.lock();
        store.get(path).cloned().ok_or_else(|| "File not found".to_string())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), String> {
        let mut store = self.storage.lock();
        let file = store.entry(path.to_string()).or_insert_with(Vec::new);
        file.extend_from_slice(data);
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        let store = self.storage.lock();
        store.contains_key(path)
    }

    fn create_dir_all(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }

    fn read_dir(&self, _path: &str) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}
