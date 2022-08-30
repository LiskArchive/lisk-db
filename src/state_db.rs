use neon::prelude::*;
use neon::types::buffer::TypedArray;
use std::cell::RefCell;
use std::cmp;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;
use thiserror::Error;

use crate::batch;
use crate::consts;
use crate::diff;
use crate::options;
use crate::smt;
use crate::smt_db;
use crate::state_writer;
use crate::utils;

#[derive(Error, Debug)]
pub enum DataStoreError {
    #[error("unknown data store error `{0}`")]
    Unknown(String),
    #[error("Diff not found for height: `{0}`")]
    DiffNotFound(u32),
}

#[derive(Clone, Debug)]
struct KVPair(Vec<u8>, Vec<u8>);

impl Finalize for StateDB {}
pub struct StateDB {
    tx: mpsc::Sender<options::DbMessage>,
    readonly: bool,
    key_length: usize,
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

        let mut opened: rocksdb::DB;
        if opts.readonly {
            opened = rocksdb::DB::open_for_read_only(&options, path, false)?;
        } else {
            opened = rocksdb::DB::open(&options, path)?;
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
            readonly: opts.readonly,
            key_length: opts.key_length,
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

    fn get_by_key(&self, key: Vec<u8>, cb: Root<JsFunction>) -> Result<(), DataStoreError> {
        self.send(move |conn, channel| {
            let result = conn.get([consts::PREFIX_STATE, key.as_slice()].concat());

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
            });
        })
        .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))
    }

    fn exists(&self, key: Vec<u8>, cb: Root<JsFunction>) -> Result<(), DataStoreError> {
        self.send(move |conn, channel| {
            let key_with_prefix = [consts::PREFIX_STATE, key.as_slice()].concat();
            let exist = conn.key_may_exist(&key_with_prefix);
            let result = if exist {
                conn.get(&key_with_prefix).and_then(|res| Ok(res.is_some()))
            } else {
                Ok(false)
            };

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let converted = ctx.boolean(val);
                        vec![ctx.null().upcast(), converted.upcast()]
                    }
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))
    }

    fn get_revert_result(
        conn: &rocksdb::DB,
        height: u32,
        state_root: Vec<u8>,
        key_length: usize,
    ) -> Result<Vec<u8>, DataStoreError> {
        let diff_bytes = conn
            .get(&[consts::PREFIX_DIFF, height.to_be_bytes().as_slice()].concat())
            .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))?
            .ok_or(DataStoreError::DiffNotFound(height))?;

        let d = diff::Diff::decode(diff_bytes)
            .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))?;
        let mut data = smt::UpdateData::new_with_hash(d.revert_update());
        let mut smtdb = smt_db::SMTDB::new(conn);
        let mut tree = smt::SMT::new(state_root, key_length, consts::SUBTREE_SIZE);
        let prev_root = tree
            .commit(&mut smtdb, &mut data)
            .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))?;

        let mut write_batch = batch::PrefixWriteBatch::new();
        // Insert state batch with diff
        write_batch.set_prefix(&consts::PREFIX_STATE);
        d.revert_commit(&mut write_batch);
        write_batch.set_prefix(&consts::PREFIX_DIFF);
        write_batch.delete(height.to_be_bytes().as_slice());

        // insert SMT batch
        write_batch.set_prefix(&consts::PREFIX_SMT);
        smtdb.batch.iterate(&mut write_batch);
        // insert diff
        conn.write(write_batch.batch)
            .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))?;

        Ok(prev_root)
    }

    fn revert(
        &mut self,
        height: u32,
        state_root: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let key_length = self.key_length;
        self.send(move |conn, channel| {
            let result = StateDB::get_revert_result(conn, height, state_root, key_length);
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
            });
        })
    }

    fn handle_commit_result(
        conn: &rocksdb::DB,
        smtdb: &smt_db::SMTDB,
        writer: MutexGuard<state_writer::StateWriter>,
        height: u32,
        readonly: bool,
        next_root: Result<Vec<u8>, smt::SMTError>,
        check_expected: bool,
        expected: &Vec<u8>,
    ) -> Result<Vec<u8>, smt::SMTError> {
        if next_root.is_err() {
            return next_root;
        }
        let root = next_root.unwrap();
        if check_expected && utils::compare(&expected, &root) != cmp::Ordering::Equal {
            return Err(smt::SMTError::InvalidRoot(String::from(
                "Not matching with expected",
            )));
        }
        if readonly {
            return Ok(root);
        }
        // Create global batch
        let mut write_batch = batch::PrefixWriteBatch::new();
        // Insert state batch with diff
        write_batch.set_prefix(&consts::PREFIX_STATE);
        let diff = writer.commit(&mut write_batch);
        write_batch.set_prefix(&consts::PREFIX_DIFF);
        write_batch.put(height.to_be_bytes().as_slice(), diff.encode().as_ref());

        // insert SMT batch
        write_batch.set_prefix(&consts::PREFIX_SMT);
        smtdb.batch.iterate(&mut write_batch);
        // insert diff
        let result = conn.write(write_batch.batch);
        match result {
            Ok(_) => Ok(root),
            Err(err) => Err(smt::SMTError::Unknown(err.to_string())),
        }
    }

    fn commit(
        &mut self,
        writer: Arc<Mutex<state_writer::StateWriter>>,
        height: u32,
        prev_root: Vec<u8>,
        readonly: bool,
        expected: Vec<u8>,
        check_expected: bool,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let key_length = self.key_length;
        self.send(move |conn, channel| {
            let w = writer.lock().unwrap();
            let mut data = smt::UpdateData::new_with_hash(w.get_updated());
            let mut smtdb = smt_db::SMTDB::new(conn);
            let mut tree = smt::SMT::new(prev_root, key_length, consts::SUBTREE_SIZE);
            let root = tree.commit(&mut smtdb, &mut data);
            let result = StateDB::handle_commit_result(
                conn,
                &smtdb,
                w,
                height,
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
            });
        })
    }

    fn prove(
        &self,
        root: Vec<u8>,
        queries: Vec<Vec<u8>>,
        cb: Root<JsFunction>,
    ) -> Result<(), DataStoreError> {
        let key_length = self.key_length;
        self.send(move |conn, channel| {
            let mut tree = smt::SMT::new(root, key_length, consts::SUBTREE_SIZE);
            let mut smtdb = smt_db::SMTDB::new(conn);
            let result = tree.prove(&mut smtdb, queries);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let obj: Handle<JsObject> = ctx.empty_object();
                        let sibling_hashes = ctx.empty_array();
                        for (i, h) in val.sibling_hashes.iter().enumerate() {
                            let val_res = JsBuffer::external(&mut ctx, h.to_vec());
                            sibling_hashes.set(&mut ctx, i as u32, val_res)?;
                        }
                        obj.set(&mut ctx, "siblingHashes", sibling_hashes)?;
                        let queries = ctx.empty_array();
                        for (i, v) in val.queries.iter().enumerate() {
                            let obj = ctx.empty_object();
                            let key = JsBuffer::external(&mut ctx, v.key.to_vec());
                            obj.set(&mut ctx, "key", key)?;
                            let value = JsBuffer::external(&mut ctx, v.value.to_vec());
                            obj.set(&mut ctx, "value", value)?;
                            let bitmap = JsBuffer::external(&mut ctx, v.bitmap.to_vec());
                            obj.set(&mut ctx, "bitmap", bitmap)?;

                            queries.set(&mut ctx, i as u32, obj)?;
                        }
                        vec![ctx.null().upcast(), obj.upcast()]
                    }
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };
                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))
    }

    fn clean_diff_until(&self, height: u32, cb: Root<JsFunction>) -> Result<(), DataStoreError> {
        if height == 0 {
            return Ok(());
        }
        self.send(move |conn, channel| {
            let zero: u32 = 0;
            let start = [consts::PREFIX_DIFF, zero.to_be_bytes().as_slice()].concat();
            let end = [consts::PREFIX_DIFF, (height - 1).to_be_bytes().as_slice()].concat();
            let mut batch = rocksdb::WriteBatch::default();

            let iter = conn.iterator(rocksdb::IteratorMode::From(
                end.as_ref(),
                rocksdb::Direction::Reverse,
            ));

            for (key, _) in iter {
                if utils::compare(&key, &start) == cmp::Ordering::Less {
                    break;
                }
                batch.delete(&key);
            }

            let result = conn.write(batch);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => {
                        vec![ctx.null().upcast()]
                    }
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };
                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| Err(DataStoreError::Unknown(err.to_string())))
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
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.get_by_key(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.exists(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_revert(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let prev_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let height = ctx.argument::<JsNumber>(1)?.value(&mut ctx) as u32;
        let cb = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.revert(height, prev_root, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let cb_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let cb_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();

        let a_cb_on_data = Arc::new(Mutex::new(cb_on_data));
        db.send(move |conn, channel| {
            let no_range = options.gte.is_none() && options.lte.is_none();
            let iter;
            if no_range {
                if options.reverse {
                    iter = conn.iterator(rocksdb::IteratorMode::End);
                } else {
                    iter = conn.iterator(rocksdb::IteratorMode::Start);
                }
            } else {
                if options.reverse {
                    let lte = options
                        .lte
                        .clone()
                        .unwrap_or_else(|| vec![255; options.gte.clone().unwrap().len()]);
                    iter = conn.iterator(rocksdb::IteratorMode::From(
                        &[consts::PREFIX_STATE, &lte].concat(),
                        rocksdb::Direction::Reverse,
                    ));
                } else {
                    let gte = options
                        .gte
                        .clone()
                        .unwrap_or_else(|| vec![0; options.lte.clone().unwrap().len()]);
                    iter = conn.iterator(rocksdb::IteratorMode::From(
                        &[consts::PREFIX_STATE, &gte].concat(),
                        rocksdb::Direction::Forward,
                    ));
                }
            }
            let mut counter = 0;
            for (key, val) in iter {
                if options.limit != -1 && counter >= options.limit {
                    break;
                }
                if options.reverse {
                    if let Some(gte) = &options.gte {
                        let prefixed_gte = &[consts::PREFIX_STATE, &gte].concat();
                        if utils::compare(&key, &prefixed_gte) == cmp::Ordering::Less {
                            break;
                        }
                    }
                } else {
                    if let Some(lte) = &options.lte {
                        let prefixed_lte = &[consts::PREFIX_STATE, &lte].concat();
                        if utils::compare(&key, &prefixed_lte) == cmp::Ordering::Greater {
                            break;
                        }
                    }
                }
                let c = a_cb_on_data.clone();
                channel.send(move |mut ctx| {
                    let obj = ctx.empty_object();
                    let (_, key_without_prefix) = key.split_first().unwrap();
                    let key_res = JsBuffer::external(&mut ctx, key_without_prefix.to_vec());
                    let val_res = JsBuffer::external(&mut ctx, val);
                    obj.set(&mut ctx, "key", key_res)?;
                    obj.set(&mut ctx, "value", val_res)?;
                    let cb = c.lock().unwrap().to_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), obj.upcast()];
                    cb.call(&mut ctx, this, args)?;
                    Ok(())
                });
                counter += 1;
            }
            channel.send(move |mut ctx| {
                let cb_2 = cb_done.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast()];
                cb_2.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    // Commit
    // @params 0 writer (required)
    // @params 1 height (required)
    // @params 2 prev_root (required)
    // @params 3 readonly
    // @params 4 expected_root
    // @params 5 check_root
    // @params 6 callback
    pub fn js_commit(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let writer = ctx.argument::<JsBox<state_writer::SendableStateWriter>>(0)?;

        let height = ctx.argument::<JsNumber>(1)?.value(&mut ctx) as u32;

        let prev_root = ctx.argument::<JsTypedArray<u8>>(2)?.as_slice(&ctx).to_vec();

        let readonly = ctx.argument::<JsBoolean>(3)?.value(&mut ctx);

        let expected = ctx.argument::<JsTypedArray<u8>>(4)?.as_slice(&ctx).to_vec();

        let check_root = ctx.argument::<JsBoolean>(5)?.value(&mut ctx);
        let cb = ctx.argument::<JsFunction>(6)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        if db.readonly {
            return ctx.throw_error(String::from("Readonly DB cannot be committed."));
        }
        let writer = writer.borrow().clone();
        db.commit(
            writer, height, prev_root, readonly, expected, check_root, cb,
        )
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_prove(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();

        let state_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();

        let input = ctx.argument::<JsArray>(1)?.to_vec(&mut ctx)?;
        let mut queries: Vec<Vec<u8>> = vec![];
        for key in input.iter() {
            let obj = key.downcast_or_throw::<JsObject, _>(&mut ctx)?;
            let key = obj
                .get::<JsTypedArray<u8>, _, _>(&mut ctx, "key")?
                .as_slice(&ctx)
                .to_vec();
            queries.push(key);
        }

        let cb = ctx.argument::<JsFunction>(2)?.root(&mut ctx);

        db.prove(state_root, queries, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_verify(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();
        let key_lengh = db.key_length;
        // root: &Vec<u8>, query_keys: &Vec<Vec<u8>>, proof: &Proof
        let state_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();

        let query_keys = ctx.argument::<JsArray>(1)?.to_vec(&mut ctx)?;
        let mut parsed_query_keys: Vec<Vec<u8>> = vec![];
        for key in query_keys.iter() {
            let key = key
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut ctx)?
                .as_slice(&ctx)
                .to_vec();
            parsed_query_keys.push(key);
        }
        let raw_proof = ctx.argument::<JsObject>(2)?;
        let mut sibling_hashes: Vec<Vec<u8>> = vec![];
        let raw_sibling_hashes = raw_proof
            .get::<JsArray, _, _>(&mut ctx, "siblingHashes")?
            .to_vec(&mut ctx)?;
        for key in raw_sibling_hashes.iter() {
            let key = key
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut ctx)?
                .as_slice(&ctx)
                .to_vec();
            sibling_hashes.push(key);
        }
        let mut queries: Vec<smt::QueryProof> = vec![];
        let raw_queries = raw_proof
            .get::<JsArray, _, _>(&mut ctx, "queries")?
            .to_vec(&mut ctx)?;
        for key in raw_queries.iter() {
            let obj = key.downcast_or_throw::<JsObject, _>(&mut ctx)?;
            let key = obj
                .get::<JsTypedArray<u8>, _, _>(&mut ctx, "key")?
                .as_slice(&ctx)
                .to_vec();
            let value = obj
                .get::<JsTypedArray<u8>, _, _>(&mut ctx, "value")?
                .as_slice(&ctx)
                .to_vec();
            let bitmap = obj
                .get::<JsTypedArray<u8>, _, _>(&mut ctx, "bitmap")?
                .as_slice(&ctx)
                .to_vec();
            queries.push(smt::QueryProof {
                key: key,
                value: value,
                bitmap: bitmap,
            });
        }
        let proof = smt::Proof {
            queries: queries,
            sibling_hashes: sibling_hashes,
        };

        let cb = ctx.argument::<JsFunction>(4)?.root(&mut ctx);

        let channel = ctx.channel();

        thread::spawn(move || {
            let result = smt::SMT::verify(&parsed_query_keys, &proof, &state_root, key_lengh);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        vec![ctx.null().upcast(), JsBoolean::new(&mut ctx, val).upcast()]
                    }
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };
                callback.call(&mut ctx, this, args)?;

                Ok(())
            })
        });

        Ok(ctx.undefined())
    }

    pub fn js_clean_diff_until(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();

        let height = ctx.argument::<JsNumber>(0)?.value(&mut ctx) as u32;

        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        db.clean_diff_until(height, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}
