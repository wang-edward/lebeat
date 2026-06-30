//! Port of `queue.zig`.
//!
//! `SpscQueue<T, N>`: `N` is the backing array length (Zig called this `CAPACITY`).
//! One slot is always left empty to disambiguate full/empty, so usable capacity is `N - 1`.
//! Zig's `SpscQueue(T, 16)` => use `SpscQueue<T, 17>` here for the same 16 usable slots.
//!
//! Same memory ordering as the Zig original: the producer stores the write index with
//! Release and reads the read index with Acquire; the consumer mirrors it.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct SpscQueue<T, const N: usize> {
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    buf: [UnsafeCell<MaybeUninit<T>>; N],
}

// Single-producer single-consumer: safe to share across the two threads.
unsafe impl<T: Send, const N: usize> Sync for SpscQueue<T, N> {}

impl<T, const N: usize> SpscQueue<T, N> {
    pub fn new() -> Self {
        Self {
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            buf: std::array::from_fn(|_| UnsafeCell::new(MaybeUninit::uninit())),
        }
    }

    /// Producer side. Returns false if the queue is full.
    pub fn push(&self, value: T) -> bool {
        let w = self.write_idx.load(Ordering::Relaxed);
        let r = self.read_idx.load(Ordering::Acquire);

        if (w + 1) % N == r {
            return false; // full
        }
        // SAFETY: SPSC invariant — only the producer writes slot `w`, and the consumer
        // cannot be reading it because the slot is currently empty.
        unsafe { (*self.buf[w].get()).write(value) };
        self.write_idx.store((w + 1) % N, Ordering::Release);
        true
    }

    /// Consumer side. Returns None if the queue is empty.
    pub fn pop(&self) -> Option<T> {
        let r = self.read_idx.load(Ordering::Relaxed);
        let w = self.write_idx.load(Ordering::Acquire);

        if r == w {
            return None; // empty
        }
        // SAFETY: slot `r` was fully written by the producer before it published `w`.
        let val = unsafe { (*self.buf[r].get()).assume_init_read() };
        self.read_idx.store((r + 1) % N, Ordering::Release);
        Some(val)
    }
}

impl<T, const N: usize> Default for SpscQueue<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for SpscQueue<T, N> {
    fn drop(&mut self) {
        // Drop any still-queued items.
        while self.pop().is_some() {}
    }
}

/// Fixed-capacity inline list, mirrors Zig `BoundedList`.
#[derive(Clone)]
pub struct BoundedList<T, const N: usize> {
    buffer: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> BoundedList<T, N> {
    pub fn new() -> Self {
        Self {
            buffer: [T::default(); N],
            len: 0,
        }
    }

    pub fn from_slice(items: &[T]) -> Self {
        debug_assert!(items.len() <= N);
        let mut s = Self::new();
        s.buffer[..items.len()].copy_from_slice(items);
        s.len = items.len();
        s
    }

    pub fn append_assume_capacity(&mut self, item: T) {
        debug_assert!(self.len < N);
        self.buffer[self.len] = item;
        self.len += 1;
    }

    pub fn as_slice(&self) -> &[T] {
        &self.buffer[..self.len]
    }
}

impl<T: Copy + Default, const N: usize> Default for BoundedList<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
