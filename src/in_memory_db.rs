use neon::prelude::*;
use neon::types::buffer::TypedArray;
use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;

use crate::batch;
use crate::options;
use crate::utils;

#[derive(Clone, Debug)]
struct KVPair(Vec<u8>, Vec<u8>);

fn sort_kv_pair(pairs: &mut Vec<KVPair>, reverse: bool) {
    if !reverse {
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        return;
    }
    pairs.sort_by(|a, b| b.0.cmp(&a.0));
}

impl Finalize for Database {}
pub struct Database {
    cache: CacheData,
}

#[derive(Clone, Debug)]
pub struct CacheData {
    data: HashMap<Vec<u8>, Vec<u8>>,
}

impl Database {
    pub fn new() -> Result<Self, rocksdb::Error> {
        return Ok(Database {
            cache: CacheData {
                data: HashMap::new(),
            },
        });
    }

    fn cache_range(&self, start: &[u8], end: &[u8]) -> Vec<KVPair> {
        self.cache
            .data
            .iter()
            .filter(|(k, _)| {
                utils::compare(k, start) != cmp::Ordering::Less
                    && utils::compare(k, end) != cmp::Ordering::Greater
            })
            .map(|(k, v)| KVPair(k.clone(), v.clone()))
            .collect()
    }

    fn cache_all(&self) -> Vec<KVPair> {
        self.cache
            .data
            .iter()
            .map(|(k, v)| KVPair(k.clone(), v.clone()))
            .collect()
    }

    fn clear(&mut self) {
        self.cache.data.clear();
    }

    fn set_kv(&mut self, key: &[u8], value: &[u8]) {
        self.cache.data.insert(key.to_vec(), value.to_vec());
    }

    fn del(&mut self, key: &[u8]) {
        self.cache.data.remove(key);
    }

    fn clone(&self) -> Self {
        let new_cache = self.cache.clone();
        Self {
            cache: new_cache,
        }
    }
}

type SharedStateDB = JsBox<RefCell<Database>>;

impl Database {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<SharedStateDB> {
        let db = Database::new().or_else(|err| ctx.throw_error(err.to_string()))?;
        let ref_db = RefCell::new(db);

        return Ok(ctx.boxed(ref_db));
    }

    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?;
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        let this = ctx.undefined();
        let args: Vec<Handle<JsValue>> = match db.cache.data.get(&key) {
            Some(val) => {
                let buffer = JsBuffer::external(&mut ctx, val.to_vec());
                vec![ctx.null().upcast(), buffer.upcast()]
            }
            None => vec![ctx.error("No data")?.upcast()],
        };
        cb.call(&mut ctx, this, args)?;

        Ok(ctx.undefined())
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let mut value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let mut db = db.borrow_mut();

        db.set_kv(&mut key, &mut value);

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.del(&mut key);

        Ok(ctx.undefined())
    }

    pub fn js_clear(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.clear();

        Ok(ctx.undefined())
    }

    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let callback = ctx.argument::<JsFunction>(1)?;
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();

        let cached;
        let no_range = options.gte.is_none() && options.lte.is_none();
        if no_range {
            cached = db.cache_all();
        } else {
            let gte = options
                .gte
                .clone()
                .unwrap_or_else(|| vec![0; options.lte.clone().unwrap().len()]);
            let lte = options.lte.clone().unwrap_or_else(|| vec![255; gte.len()]);
            cached = db.cache_range(&gte, &lte);
        }

        let mut results = vec![];
        let mut exist_map = HashMap::new();
        for kv in cached {
            exist_map.insert(kv.0.clone(), true);
            results.push(kv);
        }

        sort_kv_pair(&mut results, options.reverse);
        if options.limit != -1 && results.len() > options.limit as usize {
            results = results[..options.limit as usize].to_vec();
        }

        let this = ctx.undefined();
        let arr = JsArray::new(&mut ctx, results.len() as u32);
        for (i, kv) in results.iter().enumerate() {
            let obj = ctx.empty_object();
            let key = JsBuffer::external(&mut ctx, kv.0.clone());
            let value = JsBuffer::external(&mut ctx, kv.1.clone());
            obj.set(&mut ctx, "key", key)?;
            obj.set(&mut ctx, "value", value)?;
            arr.set(&mut ctx, i as u32, obj)?;
        }
        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), arr.upcast()];
        callback.call(&mut ctx, this, args)?;

        Ok(ctx.undefined())
    }

    pub fn js_write(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let batch = ctx.argument::<JsBox<batch::SendableWriteBatch>>(0)?;
        let cb = ctx.argument::<JsFunction>(1)?;

        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let mut db = db.borrow_mut();

        let batch = batch.borrow().clone();
        let inner_batch = batch.lock().unwrap();

        inner_batch.batch.iterate(&mut db.cache);

        let this = ctx.undefined();
        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast()];
        cb.call(&mut ctx, this, args)?;

        Ok(ctx.undefined())
    }

    pub fn js_clone(mut ctx: FunctionContext) -> JsResult<SharedStateDB> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        ctx.undefined();
        let cloned = db.clone();

        let ref_db = RefCell::new(cloned);

        return Ok(ctx.boxed(ref_db));
    }
}

impl rocksdb::WriteBatchIterator for CacheData {
    /// Called with a key and value that were `put` into the batch.
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        self.data.insert(key.to_vec(), value.to_vec());
    }
    /// Called with a key that was `delete`d from the batch.
    fn delete(&mut self, key: Box<[u8]>) {
        self.data.remove(&key.to_vec());
    }
}
