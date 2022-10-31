/// batch provides a batch feature for Database.
use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::database::traits::{DatabaseKind, JsNewWithArcMutex, NewDBWithKeyLength};
use crate::database::types::{JsArcMutex, Kind as DBKind};
use crate::types::{KVPair, KeyLength};

pub type SendableWriteBatch = JsArcMutex<WriteBatch>;

pub trait BatchWriter {
    fn put(&mut self, pair: &KVPair);
    fn delete(&mut self, key: &[u8]);
}

/// WriteBatch is a container for rocksdb::WriteBatch
pub struct WriteBatch {
    pub batch: rocksdb::WriteBatch,
}

/// PrefixWriteBatch updates rocksdb batch with defined prefix.
/// Prefix is used for splitting the data into buckets.
pub struct PrefixWriteBatch<'a> {
    pub batch: rocksdb::WriteBatch,
    prefix: Option<&'a [u8]>,
}

impl Clone for WriteBatch {
    fn clone(&self) -> Self {
        let mut cloned = WriteBatch::new_db_with_key_length(None);
        self.batch.iterate(&mut cloned);
        cloned
    }
}

impl rocksdb::WriteBatchIterator for WriteBatch {
    /// Called with a key and value that were `put` into the batch.
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        self.batch.put(key, value);
    }
    /// Called with a key that was `delete`d from the batch.
    fn delete(&mut self, key: Box<[u8]>) {
        self.batch.delete(key);
    }
}

impl NewDBWithKeyLength for WriteBatch {
    fn new_db_with_key_length(_: Option<KeyLength>) -> Self {
        Self {
            batch: rocksdb::WriteBatch::default(),
        }
    }
}

impl DatabaseKind for WriteBatch {
    fn db_kind() -> DBKind {
        DBKind::Batch
    }
}

impl JsNewWithArcMutex for WriteBatch {}
impl Finalize for WriteBatch {}
impl WriteBatch {
    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();

        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<SendableWriteBatch, _>(&mut ctx)?;

        let batch = batch.borrow();
        let mut inner_batch = batch.lock().unwrap();

        inner_batch.batch.put(key, value);

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<SendableWriteBatch, _>(&mut ctx)?;

        let batch = batch.borrow();
        let mut inner_batch = batch.lock().unwrap();

        inner_batch.batch.delete(key);

        Ok(ctx.undefined())
    }
}

impl<'a> BatchWriter for PrefixWriteBatch<'a> {
    fn put(&mut self, pair: &KVPair) {
        self.batch
            .put([self.prefix.unwrap(), pair.key()].concat(), pair.value());
    }

    fn delete(&mut self, key: &[u8]) {
        self.batch.delete([self.prefix.unwrap(), key].concat());
    }
}

impl<'a> rocksdb::WriteBatchIterator for PrefixWriteBatch<'a> {
    /// Called with a key and value that were `put` into the batch.
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        self.batch
            .put([self.prefix.unwrap(), key.as_ref()].concat(), value);
    }
    /// Called with a key that was `delete`d from the batch.
    fn delete(&mut self, key: Box<[u8]>) {
        self.batch
            .delete([self.prefix.unwrap(), key.as_ref()].concat());
    }
}

impl<'a> PrefixWriteBatch<'a> {
    pub fn new() -> Self {
        PrefixWriteBatch {
            batch: rocksdb::WriteBatch::default(),
            prefix: None,
        }
    }

    pub fn set_prefix(&mut self, prefix: &'a &[u8]) {
        self.prefix = Some(prefix);
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) {
        self.batch.put([self.prefix.unwrap(), key].concat(), value);
    }

    pub fn delete(&mut self, key: &[u8]) {
        self.batch.delete([self.prefix.unwrap(), key].concat());
    }
}

impl<'a> Default for PrefixWriteBatch<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use rocksdb::WriteBatchIterator;

    use super::*;
    use crate::consts;

    #[test]
    fn test_put_and_delete_for_write_batch() {
        let mut write_batch = WriteBatch::new_db_with_key_length(None);
        assert_eq!(write_batch.batch.len(), 0);

        write_batch.put(Box::new([1, 2, 3, 4]), Box::new([5, 6, 7, 8]));
        assert_eq!(write_batch.batch.len(), 1);

        write_batch.delete(Box::new([1, 2, 3, 4]));
        assert_eq!(write_batch.batch.len(), 2);
    }

    #[test]
    fn test_put_and_delete_for_prefix_write_batch() {
        let mut write_batch = PrefixWriteBatch::default();
        write_batch.set_prefix(&consts::Prefix::STATE);
        assert_eq!(write_batch.batch.len(), 0);

        write_batch.put(&[1, 2, 3, 4], &[5, 6, 7, 8]);
        assert_eq!(write_batch.batch.len(), 1);

        write_batch.delete(&[1, 2, 3, 4]);
        assert_eq!(write_batch.batch.len(), 2);
    }

    #[test]
    fn test_set_prefix() {
        let mut write_batch = PrefixWriteBatch::default();
        assert_eq!(write_batch.prefix, None);

        write_batch.set_prefix(&consts::Prefix::STATE);
        assert_eq!(write_batch.prefix, Some(consts::Prefix::STATE));
    }
}
