use neon::prelude::*;
use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use thiserror::Error;

use crate::batch;
use crate::options;
use crate::smt;
use crate::smt_db;

#[derive(Error, Debug)]
pub enum DataStoreError {
    #[error("fail to call callback `{0}`")]
    Callback(String),
    #[error("unknown data store error `{0}`")]
    Unknown(String),
}

const CF_STATE: &str = "state";
const KEY_LENGTH: usize = 38;
const SUBTREE_SIZE: usize = 8;

#[derive(Clone, Debug)]
struct KVPair(Vec<u8>, Vec<u8>);

fn sort_kv_pair(pairs: &mut Vec<KVPair>, reverse: bool) {
    if !reverse {
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        return;
    }
    pairs.sort_by(|a, b| b.0.cmp(&a.0));
}

impl Finalize for StateDB {}
pub struct StateDB {
    tx: mpsc::Sender<options::DbMessage>,
    backup: Option<Box<batch::WriteBatch>>,
    batch: Box<batch::WriteBatch>,
    cache: HashMap<Vec<u8>, Vec<u8>>,
    readonly: bool,
    immutable: bool,
}

impl StateDB {
    fn new<'a, C>(
        ctx: &mut C,
        path: String,
        opts: options::DatabaseOptions,
    ) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
    {
        // Channel for sending callbacks to execute on the sqlite connection thread
        let (tx, rx) = mpsc::channel::<options::DbMessage>();

        let channel = ctx.channel();

        let mut options = rocksdb::Options::default();
        options.create_if_missing(true);
        options.create_missing_column_families(true);

        let mut opened: rocksdb::DB;
        let mut cf_state_options = rocksdb::Options::default();
        cf_state_options.create_missing_column_families(true);
        let cf_state = rocksdb::ColumnFamilyDescriptor::new(CF_STATE, cf_state_options);
        let mut cf_smt_options = rocksdb::Options::default();
        cf_smt_options.create_missing_column_families(true);
        let cf_smt = rocksdb::ColumnFamilyDescriptor::new(smt_db::CF_SMT, cf_smt_options);
        if opts.readonly || opts.immutable {
            opened = rocksdb::DB::open_cf_descriptors_read_only(
                &options,
                path,
                vec![cf_state, cf_smt],
                false,
            )?;
        } else {
            opened = rocksdb::DB::open_cf_descriptors(&options, path, vec![cf_state, cf_smt])?;
        }

        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                match message {
                    options::DbMessage::Callback(f) => {
                        f(&mut opened, &channel);
                    }
                    options::DbMessage::Close => break,
                }
            }
        });

        return Ok(Self {
            tx: tx,
            backup: None,
            batch: Box::new(batch::WriteBatch::new()),
            cache: HashMap::new(),
            readonly: opts.readonly,
            immutable: opts.immutable,
        });
    }

    // Idiomatic rust would take an owned `self` to prevent use after close
    // However, it's not possible to prevent JavaScript from continuing to hold a closed database
    fn close(&self) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.tx.send(options::DbMessage::Close)
    }

    fn send(
        &self,
        callback: impl FnOnce(&mut rocksdb::DB, &Channel) + Send + 'static,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.tx
            .send(options::DbMessage::Callback(Box::new(callback)))
    }

    fn get_by_key(
        &self,
        ctx: &mut FunctionContext,
        key: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), DataStoreError> {
        if let Some(val) = self.cache.get(&key) {
            let callback = cb.into_inner(ctx);
            let buffer = JsBuffer::external(ctx, val.to_vec());
            let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), buffer.upcast()];
            let this = ctx.undefined();
            callback
                .call(ctx, this, args)
                .or_else(|err| Err(DataStoreError::Callback(err.to_string())))?;

            return Ok(());
        }
        self.send(move |conn, channel| {
            let cf = conn.cf_handle(CF_STATE).unwrap();
            let result = conn.get_cf(&cf, key);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(Some(val)) => {
                        let buffer = JsBuffer::external(&mut ctx, val);
                        vec![ctx.null().upcast(), buffer.upcast()]
                    }
                    Ok(None) => vec![ctx.error("No data")?.upcast()],
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            })
        })
        .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))
    }

    fn cache_range(&self, start: &[u8], end: &[u8]) -> Vec<KVPair> {
        self.cache
            .iter()
            .filter(|(k, _)| {
                options::compare(k, start) != cmp::Ordering::Less
                    && options::compare(k, end) != cmp::Ordering::Greater
            })
            .map(|(k, v)| KVPair(k.clone(), v.clone()))
            .collect()
    }

    fn cache_all(&self) -> Vec<KVPair> {
        self.cache
            .iter()
            .map(|(k, v)| KVPair(k.clone(), v.clone()))
            .collect()
    }

    fn clear(&mut self) {
        self.batch.clear();
    }

    fn set_kv(&mut self, key: &[u8], value: &[u8]) {
        if self.immutable {
            return;
        }
        self.cache.insert(key.to_vec(), value.to_vec());
        let batch = self.batch.as_mut();
        batch.put(key, value);
    }

    fn del(&mut self, key: &[u8]) {
        if self.immutable {
            return;
        }
        self.cache.remove(key);
        let batch = self.batch.as_mut();
        batch.delete(key);
    }

    fn snapshot(&mut self) {
        if self.immutable {
            return;
        }
        let cloned = self.batch.clone();
        self.backup = Some(cloned);
    }

    fn restore_snapshot(&mut self) {
        if self.immutable {
            return;
        }
        if let Some(batch) = &mut self.backup {
            self.batch.clone_from(batch);
        }
        self.backup = None;
    }

    fn revert(
        &mut self,
        _prev_root: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.send(move |_conn, channel| {
            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast()];

                callback.call(&mut ctx, this, args)?;

                Ok(())
            })
        })
    }

    fn handle_commit_result(
        conn: &rocksdb::DB,
        smtdb: &smt_db::SMTDB,
        statedb_batch: &mut Box<batch::WriteBatch>,
        readonly: bool,
        next_root: Result<Vec<u8>, smt::SMTError>,
        check_expected: bool,
        expected: &Vec<u8>,
    ) -> Result<Vec<u8>, smt::SMTError> {
        if next_root.is_err() {
            return next_root;
        }
        let root = next_root.unwrap();
        if check_expected && options::compare(&expected, &root) != cmp::Ordering::Equal {
            return Err(smt::SMTError::InvalidRoot(String::from(
                "Not matching with expected",
            )));
        }
        if readonly {
            return Ok(root);
        }
        let cf_state = conn.cf_handle(CF_STATE).unwrap();
        let cf_smt = conn.cf_handle(smt_db::CF_SMT).unwrap();
        let mut write_batch = batch::CfWriteBatch::new();
        write_batch.set_cf(cf_state);
        statedb_batch.iterate(&mut write_batch);
        write_batch.set_cf(cf_smt);
        smtdb.batch.iterate(&mut write_batch);
        let result = conn.write(write_batch.batch);
        match result {
            Ok(_) => Ok(root),
            Err(err) => Err(smt::SMTError::Unknown(err.to_string())),
        }
    }

    fn commit(
        &mut self,
        prev_root: Vec<u8>,
        expected: Vec<u8>,
        check_expected: bool,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let mut data = smt::UpdateData::new();
        self.batch.iterate(&mut data);
        let mut statedb_batch = self.batch.clone();
        let readonly = self.readonly;
        self.send(move |conn, channel| {
            let mut smtdb = smt_db::SMTDB::new(conn);
            let mut tree = smt::SMT::new(prev_root, KEY_LENGTH, SUBTREE_SIZE);
            let root = tree.commit(&mut smtdb, &mut data);
            let result = StateDB::handle_commit_result(
                conn,
                &smtdb,
                &mut statedb_batch,
                readonly,
                root,
                check_expected,
                &expected,
            );

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let buffer = JsBuffer::external(&mut ctx, val);
                        vec![ctx.null().upcast(), buffer.upcast()]
                    }
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            })
        })
    }
}

type SharedStateDB = JsBox<RefCell<StateDB>>;

impl StateDB {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<SharedStateDB> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let options = ctx.argument_opt(1);
        let db_opts = options::DatabaseOptions::new(&mut ctx, options)?;
        let db = StateDB::new(&mut ctx, path, db_opts)
            .or_else(|err| ctx.throw_error(err.to_string()))?;
        let ref_db = RefCell::new(db);

        return Ok(ctx.boxed(ref_db));
    }

    pub fn js_close(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        ctx.this()
            .downcast_or_throw::<SharedStateDB, _>(&mut ctx)?
            .borrow()
            .close()
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut buf, |data| data.as_slice().to_vec());
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.get_by_key(&mut ctx, key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut key_buf = ctx.argument::<JsBuffer>(0)?;
        let mut key = ctx.borrow(&mut key_buf, |data| data.as_slice().to_vec());
        let mut value_buf = ctx.argument::<JsBuffer>(1)?;
        let mut value = ctx.borrow(&mut value_buf, |data| data.as_slice().to_vec());
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let mut db = db.borrow_mut();

        db.set_kv(&mut key, &mut value);

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut key_buf = ctx.argument::<JsBuffer>(0)?;
        let mut key = ctx.borrow(&mut key_buf, |data| data.as_slice().to_vec());
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

    pub fn js_snapshot(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.snapshot();

        Ok(ctx.undefined())
    }

    pub fn js_snapshot_restore(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.restore_snapshot();

        Ok(ctx.undefined())
    }

    pub fn js_revert(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut buf, |data| data.as_slice().to_vec());
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.revert(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
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

        db.send(move |conn, channel| {
            let cf = conn.cf_handle(CF_STATE).unwrap();
            let no_range = options.gte.is_none() && options.lte.is_none();
            let iter;
            if no_range {
                if options.reverse {
                    iter = conn.iterator_cf(cf, rocksdb::IteratorMode::End);
                } else {
                    iter = conn.iterator_cf(cf, rocksdb::IteratorMode::Start);
                }
            } else {
                if options.reverse {
                    let lte = options
                        .lte
                        .clone()
                        .unwrap_or_else(|| vec![255; options.gte.clone().unwrap().len()]);
                    iter = conn.iterator_cf(
                        cf,
                        rocksdb::IteratorMode::From(&lte, rocksdb::Direction::Reverse),
                    );
                } else {
                    let gte = options
                        .gte
                        .clone()
                        .unwrap_or_else(|| vec![0; options.lte.clone().unwrap().len()]);
                    iter = conn.iterator_cf(
                        cf,
                        rocksdb::IteratorMode::From(&gte, rocksdb::Direction::Forward),
                    );
                }
            }
            let mut counter = 0;
            let mut stored_data = vec![];
            for (key, val) in iter {
                if options.limit != -1 && counter >= options.limit {
                    break;
                }
                if options.reverse {
                    if let Some(gte) = &options.gte {
                        if options::compare(&key, &gte) == cmp::Ordering::Less {
                            break;
                        }
                    }
                } else {
                    if let Some(lte) = &options.lte {
                        if options::compare(&key, &lte) == cmp::Ordering::Greater {
                            break;
                        }
                    }
                }
                stored_data.push(KVPair(key.to_vec(), val.to_vec()));
                counter += 1;
            }

            let mut results = vec![];
            let mut exist_map = HashMap::new();
            for kv in cached {
                exist_map.insert(kv.0.clone(), true);
                results.push(kv);
            }
            for kv in stored_data {
                if exist_map.get(&kv.0).is_none() {
                    results.push(kv);
                }
            }

            sort_kv_pair(&mut results, options.reverse);
            if options.limit != -1 && results.len() > options.limit as usize {
                results = results[..options.limit as usize].to_vec();
            }

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
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

                Ok(())
            })
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_commit(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut buf, |data| data.as_slice().to_vec());
        let mut expected_buf = ctx.argument::<JsBuffer>(1)?;
        let expected = ctx.borrow(&mut expected_buf, |data| data.as_slice().to_vec());
        let check_root = ctx.argument::<JsBoolean>(2)?.value(&mut ctx);
        let cb = ctx.argument::<JsFunction>(3)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        if db.immutable {
            return ctx.throw_error(String::from("Immutable DB cannot be committed."));
        }
        db.commit(key, expected, check_root, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}
