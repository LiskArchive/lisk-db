/// in_memory_db provides same interface as rocksdb for in_memory state computation or mocking rocksDB without a physical storage.
use std::cell::{RefCell, RefMut};
use std::cmp;
use std::sync::Arc;

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::batch;
use crate::database::options::IterationOption;
use crate::database::types::JsBoxRef;
use crate::types::{Cache, KVPair};
use crate::utils;

type SharedStateDB = JsBoxRef<Database>;

#[derive(Clone, Debug)]
pub struct CacheData {
    data: Cache,
}

// Database to mock the rocksdb in memory.
pub struct Database {
    cache: CacheData,
}

fn sort_key_value_pair(pairs: &mut [KVPair], reverse: bool) {
    if !reverse {
        pairs.sort_by(|a, b| a.key().cmp(b.key()));
        return;
    }
    pairs.sort_by(|a, b| b.key().cmp(a.key()));
}

fn get_key_value_pairs(db: RefMut<Database>, options: &IterationOption) -> Vec<KVPair> {
    let no_range = options.gte.is_none() && options.lte.is_none();
    let mut cached = if no_range {
        db.cache_all()
    } else {
        let gte = options
            .gte
            .clone()
            .unwrap_or_else(|| vec![0; options.lte.as_ref().unwrap().len()]);
        let lte = options.lte.clone().unwrap_or_else(|| vec![255; gte.len()]);
        db.cache_range(&gte, &lte)
    };

    sort_key_value_pair(&mut cached, options.reverse);
    if options.limit != -1 && cached.len() > options.limit as usize {
        cached = cached[..options.limit as usize].to_vec();
    }

    cached
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

impl Finalize for Database {}
impl Database {
    fn cache_range(&self, start: &[u8], end: &[u8]) -> Vec<KVPair> {
        self.cache
            .data
            .iter()
            .filter(|(k, _)| {
                utils::compare(k, start) != cmp::Ordering::Less
                    && utils::compare(k, end) != cmp::Ordering::Greater
            })
            .map(|(k, v)| KVPair::new(k, v))
            .collect()
    }

    fn cache_all(&self) -> Vec<KVPair> {
        self.cache
            .data
            .iter()
            .map(|(k, v)| KVPair::new(k, v))
            .collect()
    }

    fn clear(&mut self) {
        self.cache.data.clear();
    }

    fn set_key_value(&mut self, pair: &KVPair) {
        self.cache
            .data
            .insert(pair.key_as_vec(), pair.value_as_vec());
    }

    fn del(&mut self, key: &[u8]) {
        self.cache.data.remove(key);
    }

    fn clone(&self) -> Self {
        let new_cache = self.cache.clone();
        Self { cache: new_cache }
    }
}

impl Database {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBoxRef<Database>> {
        let db = Database {
            cache: CacheData { data: Cache::new() },
        };
        let ref_db = RefCell::new(db);

        Ok(ctx.boxed(ref_db))
    }

    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?;
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        let this = ctx.undefined();
        let args: Vec<Handle<JsValue>> = match db.cache.data.get(&key) {
            Some(val) => {
                let buffer = JsBuffer::external(&mut ctx, val.to_vec());
                vec![ctx.null().upcast(), buffer.upcast()]
            },
            None => vec![ctx.error("No data")?.upcast()],
        };
        callback.call(&mut ctx, this, args)?;

        Ok(ctx.undefined())
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let mut db = db.borrow_mut();

        db.set_key_value(&KVPair::new(&key, &value));

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.del(&key);

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
        let options = IterationOption::new(&mut ctx, option_inputs);
        let callback = ctx.argument::<JsFunction>(1)?;

        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();

        let kv_pairs = get_key_value_pairs(db, &options);

        let this = ctx.undefined();
        let arr = JsArray::new(&mut ctx, kv_pairs.len() as u32);
        for (i, kv) in kv_pairs.iter().enumerate() {
            let obj = ctx.empty_object();
            let key = JsBuffer::external(&mut ctx, kv.key_as_vec());
            let value = JsBuffer::external(&mut ctx, kv.value_as_vec());
            obj.set(&mut ctx, "key", key)?;
            obj.set(&mut ctx, "value", value)?;
            arr.set(&mut ctx, i as u32, obj)?;
        }
        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), arr.upcast()];
        callback.call(&mut ctx, this, args)?;

        Ok(ctx.undefined())
    }

    pub fn js_write(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let batch = ctx
            .argument::<batch::SendableWriteBatch>(0)?
            .downcast_or_throw::<batch::SendableWriteBatch, _>(&mut ctx)?;
        let callback = ctx.argument::<JsFunction>(1)?;

        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let mut db = db.borrow_mut();

        let batch = Arc::clone(&batch.borrow());
        let inner_batch = batch.lock().unwrap();

        inner_batch.batch.iterate(&mut db.cache);

        let this = ctx.undefined();
        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast()];
        callback.call(&mut ctx, this, args)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_cache_range() {
        let mut db = Database {
            cache: CacheData { data: Cache::new() },
        };
        db.set_key_value(&KVPair::new(&[1, 1, 1, 1], &[11, 11, 11, 11]));
        db.set_key_value(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22]));
        db.set_key_value(&KVPair::new(&[3, 3, 3, 3], &[33, 33, 33, 33]));
        db.set_key_value(&KVPair::new(&[4, 4, 4, 4], &[44, 44, 44, 44]));
        db.set_key_value(&KVPair::new(&[5, 5, 5, 5], &[55, 55, 55, 55]));
        db.set_key_value(&KVPair::new(&[6, 6, 6, 6], &[66, 66, 66, 66]));
        db.set_key_value(&KVPair::new(&[7, 7, 7, 7], &[77, 77, 77, 77]));

        let cached = db.cache_range(&[2], &[5]);

        assert_eq!(cached.len(), 3);
        assert!(cached.contains(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22])));
        assert!(cached.contains(&KVPair::new(&[3, 3, 3, 3], &[33, 33, 33, 33])));
        assert!(cached.contains(&KVPair::new(&[4, 4, 4, 4], &[44, 44, 44, 44])));
    }

    #[test]
    fn test_db_cache_all() {
        let mut db = Database {
            cache: CacheData { data: Cache::new() },
        };
        db.set_key_value(&KVPair::new(&[1, 1, 1, 1], &[11, 11, 11, 11]));
        db.set_key_value(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22]));
        db.set_key_value(&KVPair::new(&[3, 3, 3, 3], &[33, 33, 33, 33]));
        db.set_key_value(&KVPair::new(&[4, 4, 4, 4], &[44, 44, 44, 44]));
        db.set_key_value(&KVPair::new(&[5, 5, 5, 5], &[55, 55, 55, 55]));
        db.set_key_value(&KVPair::new(&[6, 6, 6, 6], &[66, 66, 66, 66]));
        db.set_key_value(&KVPair::new(&[7, 7, 7, 7], &[77, 77, 77, 77]));

        let cached = db.cache_all();

        assert_eq!(cached.len(), 7);
        for (key, value) in db.cache.data {
            assert!(cached.contains(&KVPair::new(key.as_slice(), value.as_slice())));
        }
    }

    #[test]
    fn test_db_clear() {
        let mut db = Database {
            cache: CacheData { data: Cache::new() },
        };
        db.set_key_value(&KVPair::new(&[1, 1, 1, 1], &[11, 11, 11, 11]));
        db.set_key_value(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22]));
        assert_eq!(db.cache.data.len(), 2);

        db.clear();

        assert_eq!(db.cache.data.len(), 0);
    }

    #[test]
    fn test_db_set_kv() {
        let mut db = Database {
            cache: CacheData { data: Cache::new() },
        };
        db.set_key_value(&KVPair::new(&[1, 1, 1, 1], &[11, 11, 11, 11]));
        db.set_key_value(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22]));
        assert_eq!(db.cache.data.len(), 2);

        db.set_key_value(&KVPair::new(&[3, 3, 3, 3], &[33, 33, 33, 33]));
        assert_eq!(db.cache.data.len(), 3);

        assert_eq!(
            db.cache.data.get(&[3, 3, 3, 3].to_vec()).unwrap(),
            &[33, 33, 33, 33]
        );
    }

    #[test]
    fn test_db_delete_key() {
        let mut db = Database {
            cache: CacheData { data: Cache::new() },
        };
        db.set_key_value(&KVPair::new(&[1, 1, 1, 1], &[11, 11, 11, 11]));
        db.set_key_value(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22]));
        db.set_key_value(&KVPair::new(&[3, 3, 3, 3], &[33, 33, 33, 33]));

        db.del(&[2, 2, 2, 2]);

        assert_eq!(db.cache.data.len(), 2);
        assert_eq!(db.cache.data.get(&[2, 2, 2, 2].to_vec()), None);
    }

    #[test]
    fn test_db_clone() {
        let mut db = Database {
            cache: CacheData { data: Cache::new() },
        };
        db.set_key_value(&KVPair::new(&[1, 1, 1, 1], &[11, 11, 11, 11]));
        db.set_key_value(&KVPair::new(&[2, 2, 2, 2], &[22, 22, 22, 22]));
        db.set_key_value(&KVPair::new(&[3, 3, 3, 3], &[33, 33, 33, 33]));

        let cloned = db.clone();

        assert_eq!(cloned.cache.data.len(), 3);
        for key in cloned.cache.data.keys() {
            assert!(db.cache.data.get(key).is_some());
        }
    }
}
