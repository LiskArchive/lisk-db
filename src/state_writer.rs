use neon::prelude::*;
use neon::types::buffer::TypedArray;
use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

pub type SendableStateWriter = RefCell<Arc<Mutex<StateWriter>>>;

use crate::batch;
use crate::diff;
use crate::utils;

#[derive(Error, Debug)]
pub enum StateWriterError {
    #[error("Invalid usage")]
    InvalidUsage,
}

#[derive(Clone, Debug)]
pub struct StateCache {
    init: Option<Vec<u8>>,
    value: Vec<u8>,
    dirty: bool,
    deleted: bool,
}

impl StateCache {
    fn new(val: Vec<u8>) -> Self {
        Self {
            init: None,
            value: val,
            dirty: false,
            deleted: false,
        }
    }

    fn new_existing(val: Vec<u8>) -> Self {
        let init = val.clone();
        Self {
            init: Some(init),
            value: val,
            dirty: false,
            deleted: false,
        }
    }
}

#[derive(Clone, Debug)]
struct KVPair(Vec<u8>, Vec<u8>);

trait Batch {
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>);
    fn delete(&mut self, key: Box<[u8]>);
}

pub struct StateWriter {
    counter: u32,
    pub backup: HashMap<u32, Box<HashMap<Vec<u8>, StateCache>>>,
    pub cache: HashMap<Vec<u8>, StateCache>,
}

impl Finalize for StateWriter {}

impl Clone for StateWriter {
    fn clone(&self) -> Self {
        let mut cloned = StateWriter::new();
        cloned.cache.clone_from(&self.cache);
        cloned
    }
}

impl StateWriter {
    pub fn new() -> Self {
        Self {
            counter: 0,
            backup: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    pub fn cache_new(&mut self, key: &[u8], value: &[u8]) {
        let cache = StateCache::new(value.to_vec());
        self.cache.insert(key.to_vec(), cache);
    }

    pub fn cache_existing(&mut self, key: &[u8], value: &[u8]) {
        let cache = StateCache::new_existing(value.to_vec());
        self.cache.insert(key.to_vec(), cache);
    }

    pub fn get(&self, key: &[u8]) -> (Vec<u8>, bool, bool) {
        let val = self.cache.get(key);
        if val.is_none() {
            return (vec![], false, false);
        }
        let val = val.unwrap();
        if val.deleted {
            return (vec![], true, true);
        }
        (val.value.clone(), false, true)
    }

    pub fn is_cached(&self, key: &[u8]) -> bool {
        self.cache.get(key).is_some()
    }

    fn get_range(&self, start: &[u8], end: &[u8]) -> Vec<KVPair> {
        self.cache
            .iter()
            .filter(|(k, v)| {
                utils::compare(k, start) != cmp::Ordering::Less
                    && utils::compare(k, end) != cmp::Ordering::Greater
                    && !v.deleted
            })
            .map(|(k, v)| KVPair(k.clone(), v.value.clone()))
            .collect()
    }

    pub fn update(&mut self, key: &[u8], value: &[u8]) -> Result<(), StateWriterError> {
        let mut cached = self
            .cache
            .get_mut(key)
            .ok_or(StateWriterError::InvalidUsage)?;
        cached.value = value.to_vec();
        cached.dirty = true;
        cached.deleted = false;
        Ok(())
    }

    pub fn delete(&mut self, key: &[u8]) {
        let cached = self.cache.get_mut(key);
        if cached.is_none() {
            return;
        }
        let mut cached = cached.unwrap();
        if cached.init.is_none() {
            self.cache.remove(key);
            return;
        }
        cached.deleted = true;
    }

    fn snapshot(&mut self) -> u32 {
        let cloned = Box::new(self.cache.clone());
        self.backup.insert(self.counter, cloned);
        let index = self.counter;
        self.counter += 1;
        index
    }

    fn restore_snapshot(&mut self, index: u32) -> Result<(), StateWriterError> {
        let backup = self
            .backup
            .get(&index)
            .ok_or(StateWriterError::InvalidUsage)?;
        self.cache.clone_from(backup);
        self.backup = HashMap::new();
        Ok(())
    }

    pub fn get_updated(&self) -> HashMap<Vec<u8>, Vec<u8>> {
        let mut result = HashMap::new();
        for (key, value) in self.cache.iter() {
            if value.init.is_none() || value.dirty {
                result.insert(key.clone(), value.value.clone());
                continue;
            }
            if value.deleted {
                result.insert(key.clone(), vec![]);
            }
        }
        result
    }

    pub fn commit(&self, batch: &mut impl batch::BatchWriter) -> diff::Diff {
        let mut created = vec![];
        let mut updated = vec![];
        let mut deleted = vec![];
        for (key, value) in self.cache.iter() {
            if value.init.is_none() {
                created.push(key.to_vec());
                batch.put(key, &value.value);
                continue;
            }
            if value.deleted {
                deleted.push(diff::KeyValue::new(key.to_vec(), value.value.clone()));
                batch.delete(key);
                continue;
            }
            if value.dirty {
                updated.push(diff::KeyValue::new(
                    key.to_vec(),
                    value.init.clone().unwrap().to_vec(),
                ));
                batch.put(key, &value.value);
                continue;
            }
        }
        diff::Diff::new(created, updated, deleted)
    }
}

impl StateWriter {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<SendableStateWriter>> {
        let batch = RefCell::new(Arc::new(Mutex::new(StateWriter::new())));

        Ok(ctx.boxed(batch))
    }

    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsObject> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let writer = batch.borrow().clone();
        let inner_writer = writer.lock().unwrap();

        let (value, deleted, exists) = inner_writer.get(&key);
        let obj = ctx.empty_object();
        let val_buf = JsBuffer::external(&mut ctx, value);
        obj.set(&mut ctx, "value", val_buf)?;
        let deleted_js = ctx.boolean(deleted);
        obj.set(&mut ctx, "deleted", deleted_js)?;

        let exists_js = ctx.boolean(exists);
        obj.set(&mut ctx, "exists", exists_js)?;

        Ok(obj)
    }

    pub fn js_update(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let writer = batch.borrow().clone();
        let mut inner_writer = writer.lock().unwrap();

        inner_writer
            .update(&key, &value)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_cache_new(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let writer = batch.borrow().clone();
        let mut inner_writer = writer.lock().unwrap();

        inner_writer.cache_new(&key, &value);

        Ok(ctx.undefined())
    }

    pub fn js_cache_existing(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let writer = batch.borrow().clone();
        let mut inner_writer = writer.lock().unwrap();

        inner_writer.cache_existing(&key, &value);

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let writer = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let batch = writer.borrow().clone();
        let mut inner_writer = batch.lock().unwrap();

        inner_writer.delete(&key);

        Ok(ctx.undefined())
    }

    pub fn js_is_cached(mut ctx: FunctionContext) -> JsResult<JsBoolean> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let writer = batch.borrow().clone();
        let inner_writer = writer.lock().unwrap();

        let cached = inner_writer.is_cached(&key);

        Ok(ctx.boolean(cached))
    }

    pub fn js_snapshot(mut ctx: FunctionContext) -> JsResult<JsNumber> {
        let writer = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let batch = writer.borrow().clone();
        let mut inner_writer = batch.lock().unwrap();

        let index = inner_writer.snapshot();

        Ok(ctx.number(index))
    }

    pub fn js_restore_snapshot(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let writer = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let batch = writer.borrow().clone();
        let mut inner_writer = batch.lock().unwrap();
        let index = ctx.argument::<JsNumber>(0)?.value(&mut ctx) as u32;

        match inner_writer.restore_snapshot(index) {
            Ok(()) => Ok(ctx.undefined()),
            Err(error) => ctx.throw_error(error.to_string())?,
        }
    }

    pub fn js_get_range(mut ctx: FunctionContext) -> JsResult<JsArray> {
        let start = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let end = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let batch = ctx
            .this()
            .downcast_or_throw::<JsBox<SendableStateWriter>, _>(&mut ctx)?;

        let writer = batch.borrow().clone();
        let inner_writer = writer.lock().unwrap();

        let results = inner_writer.get_range(&start, &end);
        let arr = JsArray::new(&mut ctx, results.len() as u32);
        for (i, kv) in results.iter().enumerate() {
            let obj = ctx.empty_object();
            let key = JsBuffer::external(&mut ctx, kv.0.clone());
            let value = JsBuffer::external(&mut ctx, kv.1.clone());
            obj.set(&mut ctx, "key", key)?;
            obj.set(&mut ctx, "value", value)?;
            arr.set(&mut ctx, i as u32, obj)?;
        }

        Ok(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache() {
        let mut writer = StateWriter::new();

        writer.cache_new(&[0, 0, 2], &[1, 2, 3]);
        writer.cache_existing(&[0, 0, 3], &[1, 2, 4]);

        let (value, deleted, exists) = writer.get(&[0, 0, 2]);
        assert_eq!(value, &[1, 2, 3]);
        assert_eq!(deleted, false);
        assert_eq!(exists, true);

        let (value, deleted, exists) = writer.get(&[0, 0, 3]);
        assert_eq!(value, &[1, 2, 4]);
        assert_eq!(deleted, false);
        assert_eq!(exists, true);

        let (value, deleted, exists) = writer.get(&[0, 0, 1]);
        assert_eq!(value, &[]);
        assert_eq!(deleted, false);
        assert_eq!(exists, false)
    }
}
