use neon::prelude::*;
use std::sync::{Arc, Mutex};
use std::cell::RefCell;

pub type SendableWriteBatch = RefCell<Arc<Mutex<WriteBatch>>>;

pub struct WriteBatch {
    pub batch: rocksdb::WriteBatch,
}

impl Finalize for WriteBatch {}

impl WriteBatch {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<SendableWriteBatch>> {
        let batch = RefCell::new(Arc::new(Mutex::new(WriteBatch {
            batch: rocksdb::WriteBatch::default(),
        })));

        Ok(ctx.boxed(batch))
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx
            .argument::<JsBuffer>(0)
            .map(|mut val| ctx.borrow(&mut val, |data| data.as_slice().to_vec()))?;
        let value = ctx
            .argument::<JsBuffer>(1)
            .map(|mut val| ctx.borrow(&mut val, |data| data.as_slice().to_vec()))?;
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
        let mut key_buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut key_buf, |data| data.as_slice().to_vec());
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