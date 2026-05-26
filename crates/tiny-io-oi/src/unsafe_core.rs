#[cfg(not(feature = "loom"))]
pub(crate) use core::sync::atomic::{AtomicU16, Ordering};
#[cfg(not(feature = "loom"))]
pub(crate) use core::cell::UnsafeCell;

#[cfg(feature = "loom")]
pub(crate) use loom::sync::atomic::{AtomicU16, Ordering};
#[cfg(feature = "loom")]
pub(crate) use loom::cell::UnsafeCell;

use core::marker::PhantomData;

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
    pub data: [UnsafeCell<Option<T>>; N],
    pub ref_counts: [AtomicU16; N],
}

impl<T: 'static, const N: usize> Arena<T, N> {
    #[cfg(not(feature = "loom"))]
    pub const fn new() -> Self {
        let ref_counts = [const { AtomicU16::new(0) }; N];
        Self {
            data: [const { UnsafeCell::new(None) }; N],
            ref_counts,
        }
    }

    #[cfg(feature = "loom")]
    pub fn new() -> Self {
        Self {
            data: core::array::from_fn(|_| UnsafeCell::new(None)),
            ref_counts: core::array::from_fn(|_| AtomicU16::new(0)),
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
                    #[cfg(not(feature = "loom"))]
                    {
                        *self.data[i].get() = Some(value);
                    }
                    
                    #[cfg(feature = "loom")]
                    {
                        self.data[i].with_mut(|p| {
                            *p = Some(value);
                        });
                    }

                    return Some(TinyArc::from_raw(i as u16, &self.ref_counts[i]));
                }
            }
        }
        None
    }

    pub fn get(&self, arc: &TinyArc<T>) -> Option<&T> {
        unsafe {
            #[cfg(not(feature = "loom"))]
            {
                (*self.data[arc.index()].get()).as_ref()
            }
            
            #[cfg(feature = "loom")]
            {
                let p = self.data[arc.index()].with(|p| p as *const Option<T>);
                (*p).as_ref()
            }
        }
    }
}

unsafe impl<T: 'static, const N: usize> Sync for Arena<T, N> {}
unsafe impl<T: 'static, const N: usize> Send for Arena<T, N> {}

/// Hardware Time Management (Unsafe block isolated)
static mut LOCAL_HARDWARE_TIME_US: u64 = 0;

pub struct HardwareTime;

impl HardwareTime {
    pub fn set_us(us: u64) {
        unsafe {
            LOCAL_HARDWARE_TIME_US = us;
        }
    }

    pub fn get_us() -> u64 {
        unsafe {
            LOCAL_HARDWARE_TIME_US
        }
    }
}

#[cfg(all(test, feature = "loom"))]
mod loom_tests {
    use super::*;
    use loom::thread;
    use std::sync::Arc;

    #[test]
    fn test_arena_concurrency() {
        loom::model(|| {
            let arena = Arc::new(Arena::<usize, 2>::new());

            let a1 = arena.clone();
            let t1 = thread::spawn(move || {
                if let Some(arc) = a1.alloc(42) {
                    assert_eq!(*a1.get(&arc).unwrap(), 42);
                }
            });

            let a2 = arena.clone();
            let t2 = thread::spawn(move || {
                if let Some(arc) = a2.alloc(24) {
                    assert_eq!(*a2.get(&arc).unwrap(), 24);
                }
            });

            t1.join().unwrap();
            t2.join().unwrap();
        });
    }
}
