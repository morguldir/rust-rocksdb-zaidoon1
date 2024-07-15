// Copyright 2020 Tyler Neely
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{ffi, AsColumnFamilyRef};
use libc::{c_char, c_void, size_t};
use std::slice;

/// A type alias to keep compatibility. See [`WriteBatchWithTransaction`] for details
pub type WriteBatch = WriteBatchWithTransaction<false>;

/// An atomic batch of write operations.
///
/// [`delete_range`](#method.delete_range) is not supported in [`Transaction`].
///
/// Making an atomic commit of several writes:
///
/// ```
/// use rust_rocksdb::{DB, Options, WriteBatchWithTransaction};
///
/// let path = "_path_for_rocksdb_storage1";
/// {
///     let db = DB::open_default(path).unwrap();
///     let mut batch = WriteBatchWithTransaction::<false>::default();
///     batch.put(b"my key", b"my value");
///     batch.put(b"key2", b"value2");
///     batch.put(b"key3", b"value3");
///
///     // delete_range is supported when use without transaction
///     batch.delete_range(b"key2", b"key3");
///
///     db.write(batch); // Atomically commits the batch
/// }
/// let _ = DB::destroy(&Options::default(), path);
/// ```
///
/// [`Transaction`]: crate::Transaction
pub struct WriteBatchWithTransaction<const TRANSACTION: bool> {
    pub(crate) inner: *mut ffi::rocksdb_writebatch_t,
}

/// Receives the puts and deletes of a write batch.
///
/// The application must provide an implementation of this trait when
/// iterating the operations within a `WriteBatch`
pub trait WriteBatchIterator {
    /// Called with a key and value that were `put` into the batch.
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>);
    /// Called with a key that was `delete`d from the batch.
    fn delete(&mut self, key: Box<[u8]>);
}

unsafe extern "C" fn writebatch_put_callback(
    state: *mut c_void,
    k: *const c_char,
    klen: usize,
    v: *const c_char,
    vlen: usize,
) {
    // coerce the raw pointer back into a box, but "leak" it so we prevent
    // freeing the resource before we are done with it
    let boxed_cb = Box::from_raw(state as *mut &mut dyn WriteBatchIterator);
    let leaked_cb = Box::leak(boxed_cb);
    let key = slice::from_raw_parts(k as *const u8, klen);
    let value = slice::from_raw_parts(v as *const u8, vlen);
    leaked_cb.put(
        key.to_vec().into_boxed_slice(),
        value.to_vec().into_boxed_slice(),
    );
}

unsafe extern "C" fn writebatch_delete_callback(state: *mut c_void, k: *const c_char, klen: usize) {
    // coerce the raw pointer back into a box, but "leak" it so we prevent
    // freeing the resource before we are done with it
    let boxed_cb = Box::from_raw(state as *mut &mut dyn WriteBatchIterator);
    let leaked_cb = Box::leak(boxed_cb);
    let key = slice::from_raw_parts(k as *const u8, klen);
    leaked_cb.delete(key.to_vec().into_boxed_slice());
}

impl<const TRANSACTION: bool> WriteBatchWithTransaction<TRANSACTION> {
    /// Construct with a reference to a byte array serialized by [`WriteBatch`].
    pub fn from_data(data: &[u8]) -> Self {
        unsafe {
            let ptr = data.as_ptr();
            let len = data.len();
            Self {
                inner: ffi::rocksdb_writebatch_create_from(
                    ptr as *const libc::c_char,
                    len as size_t,
                ),
            }
        }
    }

    pub fn len(&self) -> usize {
        unsafe { ffi::rocksdb_writebatch_count(self.inner) as usize }
    }

    /// Return WriteBatch serialized size (in bytes).
    pub fn size_in_bytes(&self) -> usize {
        unsafe {
            let mut batch_size: size_t = 0;
            ffi::rocksdb_writebatch_data(self.inner, &mut batch_size);
            batch_size
        }
    }

    /// Return a reference to a byte array which represents a serialized version of the batch.
    pub fn data(&self) -> &[u8] {
        unsafe {
            let mut batch_size: size_t = 0;
            let batch_data = ffi::rocksdb_writebatch_data(self.inner, &mut batch_size);
            std::slice::from_raw_parts(batch_data as _, batch_size)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate the put and delete operations within this write batch. Note that
    /// this does _not_ return an `Iterator` but instead will invoke the `put()`
    /// and `delete()` member functions of the provided `WriteBatchIterator`
    /// trait implementation.
    pub fn iterate(&self, callbacks: &mut dyn WriteBatchIterator) {
        let state = Box::into_raw(Box::new(callbacks));
        unsafe {
            ffi::rocksdb_writebatch_iterate(
                self.inner,
                state as *mut c_void,
                Some(writebatch_put_callback),
                Some(writebatch_delete_callback),
            );
            // we must manually set the raw box free since there is no
            // associated "destroy" callback for this object
            drop(Box::from_raw(state));
        }
    }

    /// Insert a value into the database under the given key.
    pub fn put<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();

        unsafe {
            ffi::rocksdb_writebatch_put(
                self.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            );
        }
    }

    /// Insert a value into the specific column family of the database under the given key.
    pub fn put_cf<K, V>(&mut self, cf: &impl AsColumnFamilyRef, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();

        unsafe {
            ffi::rocksdb_writebatch_put_cf(
                self.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            );
        }
    }

    /// Insert a value into the specific column family of the database
    /// under the given key with timestamp.
    pub fn put_cf_with_ts<K, V, S>(&mut self, cf: &impl AsColumnFamilyRef, key: K, ts: S, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
        S: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();
        let ts = ts.as_ref();
        unsafe {
            ffi::rocksdb_writebatch_put_cf_with_ts(
                self.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                ts.as_ptr() as *const c_char,
                ts.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            );
        }
    }

    pub fn merge<K, V>(&mut self, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();

        unsafe {
            ffi::rocksdb_writebatch_merge(
                self.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            );
        }
    }

    pub fn merge_cf<K, V>(&mut self, cf: &impl AsColumnFamilyRef, key: K, value: V)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let key = key.as_ref();
        let value = value.as_ref();

        unsafe {
            ffi::rocksdb_writebatch_merge_cf(
                self.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                value.as_ptr() as *const c_char,
                value.len() as size_t,
            );
        }
    }

    /// Removes the database entry for key. Does nothing if the key was not found.
    pub fn delete<K: AsRef<[u8]>>(&mut self, key: K) {
        let key = key.as_ref();

        unsafe {
            ffi::rocksdb_writebatch_delete(
                self.inner,
                key.as_ptr() as *const c_char,
                key.len() as size_t,
            );
        }
    }

    /// Removes the database entry in the specific column family for key.
    /// Does nothing if the key was not found.
    pub fn delete_cf<K: AsRef<[u8]>>(&mut self, cf: &impl AsColumnFamilyRef, key: K) {
        let key = key.as_ref();

        unsafe {
            ffi::rocksdb_writebatch_delete_cf(
                self.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
            );
        }
    }

    /// Removes the database entry in the specific column family with timestamp for key.
    /// Does nothing if the key was not found.
    pub fn delete_cf_with_ts<K: AsRef<[u8]>, S: AsRef<[u8]>>(
        &mut self,
        cf: &impl AsColumnFamilyRef,
        key: K,
        ts: S,
    ) {
        let key = key.as_ref();
        let ts = ts.as_ref();
        unsafe {
            ffi::rocksdb_writebatch_delete_cf_with_ts(
                self.inner,
                cf.inner(),
                key.as_ptr() as *const c_char,
                key.len() as size_t,
                ts.as_ptr() as *const c_char,
                ts.len() as size_t,
            );
        }
    }

    /// Clear all updates buffered in this batch.
    pub fn clear(&mut self) {
        unsafe {
            ffi::rocksdb_writebatch_clear(self.inner);
        }
    }
}

impl WriteBatchWithTransaction<false> {
    /// Remove database entries from start key to end key.
    ///
    /// Removes the database entries in the range ["begin_key", "end_key"), i.e.,
    /// including "begin_key" and excluding "end_key". It is not an error if no
    /// keys exist in the range ["begin_key", "end_key").
    pub fn delete_range<K: AsRef<[u8]>>(&mut self, from: K, to: K) {
        let (start_key, end_key) = (from.as_ref(), to.as_ref());

        unsafe {
            ffi::rocksdb_writebatch_delete_range(
                self.inner,
                start_key.as_ptr() as *const c_char,
                start_key.len() as size_t,
                end_key.as_ptr() as *const c_char,
                end_key.len() as size_t,
            );
        }
    }

    /// Remove database entries in column family from start key to end key.
    ///
    /// Removes the database entries in the range ["begin_key", "end_key"), i.e.,
    /// including "begin_key" and excluding "end_key". It is not an error if no
    /// keys exist in the range ["begin_key", "end_key").
    pub fn delete_range_cf<K: AsRef<[u8]>>(&mut self, cf: &impl AsColumnFamilyRef, from: K, to: K) {
        let (start_key, end_key) = (from.as_ref(), to.as_ref());

        unsafe {
            ffi::rocksdb_writebatch_delete_range_cf(
                self.inner,
                cf.inner(),
                start_key.as_ptr() as *const c_char,
                start_key.len() as size_t,
                end_key.as_ptr() as *const c_char,
                end_key.len() as size_t,
            );
        }
    }
}

impl<const TRANSACTION: bool> Default for WriteBatchWithTransaction<TRANSACTION> {
    fn default() -> Self {
        Self {
            inner: unsafe { ffi::rocksdb_writebatch_create() },
        }
    }
}

impl<const TRANSACTION: bool> Drop for WriteBatchWithTransaction<TRANSACTION> {
    fn drop(&mut self) {
        unsafe {
            ffi::rocksdb_writebatch_destroy(self.inner);
        }
    }
}

unsafe impl<const TRANSACTION: bool> Send for WriteBatchWithTransaction<TRANSACTION> {}
