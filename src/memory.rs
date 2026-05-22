extern crate alloc;

use core::sync::atomic::{AtomicU16, Ordering};
use core::marker::PhantomData;
use core::cell::UnsafeCell;

/// A simple reference-counted handle that indices into a static arena.
/// This avoids using real pointers and is safe for no_std environments.
#[derive(Debug)]
pub struct TinyArc<T> {
    index: u16,
    // Pointer to the reference count in the arena
    ref_count_ptr: *const AtomicU16,
    _marker: PhantomData<T>,
}

// Manually implement Send/Sync for TinyArc
unsafe impl<T> Send for TinyArc<T> {}
unsafe impl<T> Sync for TinyArc<T> {}

impl<T> TinyArc<T> {
    /// Safety: Caller must ensure the index and ref_count_ptr are valid for the lifetime of the arena.
    pub unsafe fn new(index: u16, ref_count_ptr: *const AtomicU16) -> Self {
        unsafe {
            (*ref_count_ptr).fetch_add(1, Ordering::SeqCst);
        }
        Self {
            index,
            ref_count_ptr,
            _marker: PhantomData,
        }
    }

    /// Safety: Caller must ensure the index and ref_count_ptr are valid, and the slot is already reserved.
    pub unsafe fn from_raw(index: u16, ref_count_ptr: *const AtomicU16) -> Self {
        Self {
            index,
            ref_count_ptr,
            _marker: PhantomData,
        }
    }

    pub fn index(&self) -> usize {
        self.index as usize
    }
}

impl<T> Clone for TinyArc<T> {
    fn clone(&self) -> Self {
        unsafe {
            (*self.ref_count_ptr).fetch_add(1, Ordering::SeqCst);
        }
        Self {
            index: self.index,
            ref_count_ptr: self.ref_count_ptr,
            _marker: PhantomData,
        }
    }
}

impl<T> Drop for TinyArc<T> {
    fn drop(&mut self) {
        unsafe {
            (*self.ref_count_ptr).fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A fixed-size memory arena for storing state records.
pub struct Arena<T: 'static, const N: usize> {
    pub data: UnsafeCell<[Option<T>; N]>,
    pub ref_counts: [AtomicU16; N],
}

impl<T: 'static, const N: usize> Arena<T, N> {
    pub const fn new() -> Self {
        let ref_counts = [const { AtomicU16::new(0) }; N];
        Self {
            data: UnsafeCell::new([const { None }; N]),
            ref_counts,
        }
    }

    pub fn alloc(&self, value: T) -> Option<TinyArc<T>> {
        for i in 0..N {
            // Atomically transition ref_count from 0 to 1 to reserve the slot
            if self.ref_counts[i]
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                unsafe {
                    let data_ptr = self.data.get() as *mut Option<T>;
                    (*data_ptr.add(i)) = Some(value);
                    return Some(TinyArc::from_raw(i as u16, &self.ref_counts[i]));
                }
            }
        }
        None
    }

    pub fn get(&self, arc: &TinyArc<T>) -> Option<&T> {
        unsafe {
            let data_ptr = self.data.get() as *const Option<T>;
            (*data_ptr.add(arc.index())).as_ref()
        }
    }
}

unsafe impl<T: 'static, const N: usize> Sync for Arena<T, N> {}
unsafe impl<T: 'static, const N: usize> Send for Arena<T, N> {}

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
