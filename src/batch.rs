use neon::prelude::*;
use neon::types::buffer::TypedArray;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

pub type SendableWriteBatch = RefCell<Arc<Mutex<WriteBatch>>>;

pub struct WriteBatch {
    pub batch: rocksdb::WriteBatch,
}

impl Finalize for WriteBatch {}

impl WriteBatch {
    pub fn new() -> Self {
        Self {
            batch: rocksdb::WriteBatch::default(),
        }
    }

    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<SendableWriteBatch>> {
        let batch = RefCell::new(Arc::new(Mutex::new(WriteBatch {
            batch: rocksdb::WriteBatch::default(),
        })));

        Ok(ctx.boxed(batch))
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();

        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableWriteBatch>, _>(&mut ctx)?;

        let batch = batch.borrow().clone();
        let mut inner_batch = batch.lock().unwrap();

        inner_batch.batch.put(key, value);

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableWriteBatch>, _>(&mut ctx)?;

        let batch = batch.borrow().clone();
        let mut inner_batch = batch.lock().unwrap();

        inner_batch.batch.delete(key);

        Ok(ctx.undefined())
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

impl Clone for WriteBatch {
    fn clone(&self) -> Self {
        let mut cloned = WriteBatch::new();
        self.batch.iterate(&mut cloned);
        cloned
    }
}

pub struct PrefixWriteBatch<'a> {
    pub batch: rocksdb::WriteBatch,
    prefix: Option<&'a [u8]>,
}

pub trait BatchWriter {
    fn put(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
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
        self.batch
            .put([self.prefix.unwrap(), key.as_ref()].concat(), value);
    }

    pub fn delete(&mut self, key: &[u8]) {
        self.batch
            .delete([self.prefix.unwrap(), key.as_ref()].concat());
    }
}

impl<'a> BatchWriter for PrefixWriteBatch<'a> {
    fn put(&mut self, key: &[u8], value: &[u8]) {
        self.batch
            .put([self.prefix.unwrap(), key.as_ref()].concat(), value);
    }

    fn delete(&mut self, key: &[u8]) {
        self.batch
            .delete([self.prefix.unwrap(), key.as_ref()].concat());
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
