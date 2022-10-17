use std::cmp;
use std::convert::TryInto;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;

use neon::prelude::*;
use neon::types::buffer::TypedArray;
use thiserror::Error;

use crate::batch;
use crate::common_db::{JsBoxRef, JsNewWithBoxRef, Kind as DBKind, NewDBWithContext, DB};
use crate::consts;
use crate::diff;
use crate::options;
use crate::smt::{self, EMPTY_HASH};
use crate::smt_db;
use crate::state_writer;
use crate::types::{
    ArcMutex, BlockHeight, Cache, CommitOptions, DbOptions, KVPair, KeyLength, NestedVec,
    SharedKVPair, SharedVec,
};
use crate::utils;

type SharedStateDB = JsBoxRef<StateDB>;

#[derive(Error, Debug)]
pub enum DataStoreError {
    #[error("unknown data store error `{0}`")]
    Unknown(String),
    #[error("Diff not found for height: `{0}`")]
    DiffNotFound(usize),
}

#[derive(Debug, PartialEq, Eq)]
struct CurrentState<'a> {
    root: &'a [u8],
    version: BlockHeight,
}

struct Commit {
    options: CommitOptions,
    check_expected: bool,
    expected: Vec<u8>,
}

struct CommitData {
    data: Commit,
    prev_root: Vec<u8>,
}

struct CommitResultInfo {
    next_root: Result<SharedVec, smt::SMTError>,
    data: Commit,
}

pub struct StateDB {
    common: DB,
    options: DbOptions,
}

impl<'a> CurrentState<'a> {
    fn new(root: &'a [u8], version: BlockHeight) -> Self {
        Self { root, version }
    }

    fn to_bytes(&self) -> Vec<u8> {
        [self.root, &self.version.to_be_bytes()].concat()
    }

    fn from_bytes(bytes: &'a [u8]) -> Self {
        let version_point = bytes.len() - 4;
        let root = &bytes[..version_point];
        let version = u32::from_be_bytes(bytes[version_point..].try_into().unwrap()).into();
        Self { root, version }
    }
}

impl Commit {
    fn new(expected: Vec<u8>, options: CommitOptions, check_expected: bool) -> Self {
        Self {
            options,
            check_expected,
            expected,
        }
    }
}

impl CommitData {
    fn new(data: Commit, prev_root: Vec<u8>) -> Self {
        Self { data, prev_root }
    }
}

impl CommitResultInfo {
    fn new(next_root: Result<SharedVec, smt::SMTError>, data: Commit) -> Self {
        Self { data, next_root }
    }
}

impl NewDBWithContext for StateDB {
    fn new_db_with_context<'a, C>(
        ctx: &mut C,
        path: String,
        db_options: DbOptions,
        k: DBKind,
    ) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
    {
        Ok(Self {
            common: DB::new_db_with_context(ctx, path, db_options, k)?,
            options: db_options,
        })
    }
}

impl JsNewWithBoxRef for StateDB {}
impl Finalize for StateDB {}
impl StateDB {
    fn get_revert_result(
        conn: &rocksdb::DB,
        version: BlockHeight,
        state_root: &[u8],
        key_length: KeyLength,
    ) -> Result<SharedVec, DataStoreError> {
        let diff_bytes = conn
            .get(&[consts::Prefix::DIFF, &version.to_be_bytes()].concat())
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?
            .ok_or_else(|| DataStoreError::DiffNotFound(version.into()))?;

        let d = diff::Diff::decode(&diff_bytes)
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?;
        let data = smt::UpdateData::new_with_hash(d.revert_update());
        let mut smtdb = smt_db::SmtDB::new(conn);
        let mut tree = smt::SparseMerkleTree::new(state_root, key_length, consts::SUBTREE_HEIGHT);
        let prev_root = tree
            .commit(&mut smtdb, &data)
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?;

        let mut write_batch = batch::PrefixWriteBatch::new();
        // Insert state batch with diff
        write_batch.set_prefix(&consts::Prefix::STATE);
        d.revert_commit(&mut write_batch);
        write_batch.set_prefix(&consts::Prefix::DIFF);
        write_batch.delete(&version.to_be_bytes());

        // insert SMT batch
        write_batch.set_prefix(&consts::Prefix::SMT);
        smtdb.batch.iterate(&mut write_batch);
        // insert diff
        conn.write(write_batch.batch)
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?;

        Ok(prev_root)
    }

    fn revert(
        &mut self,
        version: BlockHeight,
        state_root: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let key_length = self.options.key_length();
        self.common.send(move |conn, channel| {
            let result = StateDB::get_revert_result(conn, version, &state_root, key_length);
            if result.is_ok() {
                let value = (**result.as_ref().unwrap().lock().unwrap()).clone();
                let state_info = CurrentState::new(&value, version.sub(1));
                conn.put(consts::Prefix::CURRENT_STATE, state_info.to_bytes())
                    .expect("Update state info should not be failed");
            }
            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let buffer = JsBuffer::external(&mut ctx, (**val.lock().unwrap()).clone());
                        vec![ctx.null().upcast(), buffer.upcast()]
                    },
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    fn handle_commit_result(
        conn: &rocksdb::DB,
        smtdb: &smt_db::SmtDB,
        writer: MutexGuard<state_writer::StateWriter>,
        info: CommitResultInfo,
    ) -> Result<SharedVec, smt::SMTError> {
        info.next_root.as_ref()?;
        let root = info.next_root.unwrap();
        if info.data.check_expected
            && utils::compare(&info.data.expected, &root.lock().unwrap()) != cmp::Ordering::Equal
        {
            return Err(smt::SMTError::InvalidRoot(String::from(
                "Not matching with expected",
            )));
        }
        if info.data.options.is_readonly() {
            return Ok(root);
        }
        // Create global batch
        let mut write_batch = batch::PrefixWriteBatch::new();
        // Insert state batch with diff
        write_batch.set_prefix(&consts::Prefix::STATE);
        let diff = writer.commit(&mut write_batch);
        write_batch.set_prefix(&consts::Prefix::DIFF);
        let key = info.data.options.version().to_be_bytes();
        write_batch.put(&key, diff.encode().as_ref());

        // insert SMT batch
        write_batch.set_prefix(&consts::Prefix::SMT);
        smtdb.batch.iterate(&mut write_batch);
        // insert diff
        let result = conn.write(write_batch.batch);
        let version = info.data.options.version();
        match result {
            Ok(_) => {
                let value = (**root.as_ref().lock().unwrap()).clone();
                let state_info = CurrentState::new(&value, version);
                conn.put(consts::Prefix::CURRENT_STATE, state_info.to_bytes())
                    .expect("Update state info should not be failed");
                Ok(root)
            },
            Err(err) => Err(smt::SMTError::Unknown(err.to_string())),
        }
    }

    fn commit(
        &mut self,
        writer: ArcMutex<state_writer::StateWriter>,
        d: CommitData,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let key_length = self.options.key_length();
        self.common.send(move |conn, channel| {
            let w = writer.lock().unwrap();
            let data = smt::UpdateData::new_with_hash(w.get_updated());
            let mut smtdb = smt_db::SmtDB::new(conn);
            let mut tree =
                smt::SparseMerkleTree::new(&d.prev_root, key_length, consts::SUBTREE_HEIGHT);
            let root = tree.commit(&mut smtdb, &data);
            let result_info = CommitResultInfo::new(root, d.data);
            let result = StateDB::handle_commit_result(conn, &smtdb, w, result_info);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let buffer = JsBuffer::external(&mut ctx, (**val.lock().unwrap()).clone());
                        vec![ctx.null().upcast(), buffer.upcast()]
                    },
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
        queries: NestedVec,
        cb: Root<JsFunction>,
    ) -> Result<(), DataStoreError> {
        let key_length = self.options.key_length();
        self.common
            .send(move |conn, channel| {
                let mut tree =
                    smt::SparseMerkleTree::new(&root, key_length, consts::SUBTREE_HEIGHT);
                let mut smtdb = smt_db::SmtDB::new(conn);
                let result = tree.prove(&mut smtdb, &queries);

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
                                let obj = Self::pair_to_js_object(&mut ctx, &v.pair)?;
                                let bitmap = JsBuffer::external(&mut ctx, v.bitmap.to_vec());
                                obj.set(&mut ctx, "bitmap", bitmap)?;

                                queries.set(&mut ctx, i as u32, obj)?;
                            }
                            obj.set(&mut ctx, "queries", queries)?;
                            vec![ctx.null().upcast(), obj.upcast()]
                        },
                        Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                    };
                    callback.call(&mut ctx, this, args)?;

                    Ok(())
                });
            })
            .map_err(|err| DataStoreError::Unknown(err.to_string()))
    }

    fn clean_diff_until(
        &self,
        version: BlockHeight,
        cb: Root<JsFunction>,
    ) -> Result<(), DataStoreError> {
        if version.is_equal_to(0) {
            return Ok(());
        }
        self.common
            .send(move |conn, channel| {
                let zero: u32 = 0;
                let start = [consts::Prefix::DIFF, zero.to_be_bytes().as_slice()].concat();
                let end = [consts::Prefix::DIFF, &version.sub(1).to_be_bytes()].concat();
                let mut batch = rocksdb::WriteBatch::default();

                let iter = conn.iterator(rocksdb::IteratorMode::From(
                    end.as_ref(),
                    rocksdb::Direction::Reverse,
                ));

                for key_val in iter {
                    if utils::compare(&(key_val.as_ref().unwrap().0), &start)
                        == cmp::Ordering::Less
                    {
                        break;
                    }
                    batch.delete(&(key_val.unwrap().0));
                }

                let result = conn.write(batch);

                channel.send(move |mut ctx| {
                    let callback = cb.into_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args: Vec<Handle<JsValue>> = match result {
                        Ok(_) => {
                            vec![ctx.null().upcast()]
                        },
                        Err(err) => vec![ctx.error(&err)?.upcast()],
                    };
                    callback.call(&mut ctx, this, args)?;

                    Ok(())
                });
            })
            .map_err(|err| DataStoreError::Unknown(err.to_string()))
    }

    fn proof(ctx: &mut FunctionContext) -> NeonResult<smt::Proof> {
        let raw_proof = ctx.argument::<JsObject>(2)?;
        let raw_sibling_hashes = raw_proof
            .get::<JsArray, _, _>(ctx, "siblingHashes")?
            .to_vec(ctx)?;
        let sibling_hashes = raw_sibling_hashes
            .iter()
            .map(|key| {
                Ok(key
                    .downcast_or_throw::<JsTypedArray<u8>, _>(ctx)?
                    .as_slice(ctx)
                    .to_vec())
            })
            .collect::<NeonResult<NestedVec>>()?;

        let raw_queries = raw_proof
            .get::<JsArray, _, _>(ctx, "queries")?
            .to_vec(ctx)?;
        let mut queries: Vec<smt::QueryProof> = Vec::with_capacity(raw_queries.len());
        for key in raw_queries.iter() {
            let obj = key.downcast_or_throw::<JsObject, _>(ctx)?;
            let key = obj
                .get::<JsTypedArray<u8>, _, _>(ctx, "key")?
                .as_slice(ctx)
                .to_vec();
            let value = obj
                .get::<JsTypedArray<u8>, _, _>(ctx, "value")?
                .as_slice(ctx)
                .to_vec();
            let bitmap = obj
                .get::<JsTypedArray<u8>, _, _>(ctx, "bitmap")?
                .as_slice(ctx)
                .to_vec();
            queries.push(smt::QueryProof {
                pair: Arc::new(KVPair::new(&key, &value)),
                bitmap: Arc::new(bitmap),
            });
        }

        Ok(smt::Proof {
            queries,
            sibling_hashes,
        })
    }

    fn parse_query_keys(ctx: &mut FunctionContext) -> NeonResult<NestedVec> {
        let query_keys = ctx.argument::<JsArray>(1)?.to_vec(ctx)?;
        let parsed_query_keys = query_keys
            .iter()
            .map(|key| {
                Ok(key
                    .downcast_or_throw::<JsTypedArray<u8>, _>(ctx)?
                    .as_slice(ctx)
                    .to_vec())
            })
            .collect::<NeonResult<NestedVec>>()?;

        Ok(parsed_query_keys)
    }

    fn get_current_state(
        &self,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.common.send(move |conn, channel| {
            let result = conn.get(consts::Prefix::CURRENT_STATE);
            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(value) => {
                        let temp_value: Vec<u8>;
                        let current_state_info = if let Some(value) = value {
                            temp_value = value;
                            CurrentState::from_bytes(&temp_value)
                        } else {
                            CurrentState::new(&EMPTY_HASH, BlockHeight(0))
                        };
                        let root = JsBuffer::external(&mut ctx, current_state_info.root.to_vec());
                        let version = ctx.number::<u32>(current_state_info.version.into());
                        let object = ctx.empty_object();
                        object.set(&mut ctx, "root", root)?;
                        object.set(&mut ctx, "version", version)?;
                        vec![ctx.null().upcast(), object.upcast()]
                    },
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };
                callback.call(&mut ctx, this, args)?;
                Ok(())
            });
        })
    }

    fn parse_update_result<'a, C: Context<'a>>(
        ctx: &mut C,
        result: Result<(), state_writer::StateWriterError>,
    ) -> NeonResult<Vec<Handle<'a, JsValue>>> {
        if result.is_err() {
            let err = result.err().unwrap().to_string();
            Ok(vec![ctx.error(err)?.upcast()])
        } else {
            Ok(vec![ctx.null().upcast()])
        }
    }

    // update or insert the pair of key and value
    fn upsert_key(
        &self,
        cb: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        key: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let state_db_key = DBKind::State.key(key.clone());
        self.common.send(move |conn, channel| {
            let value = conn.get(&state_db_key);
            channel.send(move |mut ctx| {
                let args = {
                    let mut writer = writer.lock().unwrap();
                    let cached = writer.is_cached(&key);
                    if cached {
                        //  if the key already in cache so update it and returns
                        let result = writer.update(&KVPair::new(&key, &new_value));
                        Self::parse_update_result(&mut ctx, result)?
                    } else if let Ok(value) = &value {
                        // if found the value of the key then insert into cache and update it
                        if value.is_some() {
                            let temp_value = value.as_ref().unwrap().to_vec();
                            let pair = SharedKVPair::new(&key, &temp_value);
                            writer.cache_existing(&pair);
                            let result = writer.update(&KVPair::new(&key, &new_value));
                            Self::parse_update_result(&mut ctx, result)?
                        } else {
                            // if there is no key then make a new pair and insert into cache
                            writer.cache_new(&SharedKVPair::new(&key, &new_value));
                            vec![ctx.null().upcast()]
                        }
                    } else {
                        let err = value.err().unwrap();
                        vec![ctx.error(&err)?.upcast()]
                    }
                };

                let this = ctx.undefined();
                let callback = cb.into_inner(&mut ctx);
                callback.call(&mut ctx, this, args)?;
                Ok(())
            });
        })
    }

    fn delete_key(
        &self,
        cb: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        key: Vec<u8>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let state_db_key = DBKind::State.key(key.clone());
        self.common.send(move |conn, channel| {
            let value = conn.get(&state_db_key);
            channel.send(move |mut ctx| {
                let this = ctx.undefined();
                let callback = cb.into_inner(&mut ctx);
                // the following scope use to release writer at the end of it
                {
                    let mut writer = writer.lock().unwrap();
                    let cached = writer.is_cached(&key);
                    if !cached {
                        if let Ok(value) = &value {
                            // if found the value of the key then insert into cache
                            if value.is_some() {
                                let temp_value = value.as_ref().unwrap().to_vec();
                                let pair = SharedKVPair::new(&key, &temp_value);
                                writer.cache_existing(&pair);
                            }
                        } else {
                            let err = value.err().unwrap();
                            let args = vec![ctx.error(&err)?.upcast()];
                            callback.call(&mut ctx, this, args)?;
                        }
                    }
                    writer.delete(&key);
                }
                let args = vec![ctx.null().upcast()];
                callback.call(&mut ctx, this, args)?;
                Ok(())
            });
        })
    }

    fn get_key_with_writer(
        &self,
        cb: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        key: Vec<u8>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        let state_db_key = DBKind::State.key(key.clone());
        self.common.send(move |conn, channel| {
            let value = conn.get(&state_db_key);
            channel.send(move |mut ctx| {
                let args = {
                    let mut writer = writer.lock().unwrap();
                    let (cached_value, deleted, exists) = writer.get(&key);
                    if exists && !deleted {
                        let buffer = JsBuffer::external(&mut ctx, cached_value);
                        vec![ctx.null().upcast(), buffer.upcast()]
                    } else if deleted {
                        vec![ctx.error("No data")?.upcast()]
                    } else if let Ok(value) = &value {
                        // if found the value of the key then insert into cache
                        if value.is_some() {
                            let temp_value = value.as_ref().unwrap().to_vec();
                            let pair = SharedKVPair::new(&key, &temp_value);
                            writer.cache_existing(&pair);
                            let buffer = JsBuffer::external(&mut ctx, temp_value);
                            vec![ctx.null().upcast(), buffer.upcast()]
                        } else {
                            vec![ctx.error("No data")?.upcast()]
                        }
                    } else {
                        let err = value.err().unwrap();
                        vec![ctx.error(&err)?.upcast()]
                    }
                };

                let this = ctx.undefined();
                let callback = cb.into_inner(&mut ctx);
                callback.call(&mut ctx, this, args)?;
                Ok(())
            });
        })
    }

    fn cache_to_js_array<'a, C: Context<'a>>(
        ctx: &mut C,
        values: &Cache,
    ) -> NeonResult<Handle<'a, JsArray>> {
        let res_values = ctx.empty_array();
        for (i, kv) in values.iter().enumerate() {
            let temp_pair = KVPair::new(kv.0, kv.1);
            let object = Self::pair_to_js_object(ctx, &temp_pair)?;
            res_values.set(ctx, i as u32, object)?;
        }

        Ok(res_values)
    }

    fn pair_to_js_object<'a, C: Context<'a>>(
        ctx: &mut C,
        pair: &KVPair,
    ) -> NeonResult<Handle<'a, JsObject>> {
        let obj = ctx.empty_object();
        let key = JsBuffer::external(ctx, pair.key_as_vec());
        obj.set(ctx, "key", key)?;
        let value = JsBuffer::external(ctx, pair.value_as_vec());
        obj.set(ctx, "value", value)?;

        Ok(obj)
    }

    fn range(
        &self,
        cb: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        options: options::IterationOption,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.common.send(move |conn, channel| {
            let values = conn
                .iterator(utils::get_iteration_mode(&options, &mut vec![], true))
                .map(|key_val| {
                    KVPair::new(&key_val.as_ref().unwrap().0.clone(), &key_val.unwrap().1)
                })
                .collect::<Vec<KVPair>>();
            channel.send(move |mut ctx| {
                let result = {
                    let mut writer = writer.lock().unwrap();
                    let mut result = writer.get_range(&options);
                    for (counter, pair) in values.iter().enumerate() {
                        if utils::is_key_out_of_range(&options, pair.key(), counter as i64, true) {
                            break;
                        }
                        let (_, key_without_prefix) = pair.key().split_first().unwrap();
                        let (cached_value, deleted, exists) = writer.get(key_without_prefix);
                        if exists && !deleted {
                            result.insert(key_without_prefix.to_vec(), cached_value);
                        } else if deleted {
                            continue;
                        } else {
                            let shared_pair = SharedKVPair::new(pair.key(), pair.value());
                            writer.cache_existing(&shared_pair);
                            result.insert(pair.key_as_vec(), pair.value_as_vec());
                        }
                    }
                    Self::cache_to_js_array(&mut ctx, &result)?
                };
                let this = ctx.undefined();
                let callback = cb.into_inner(&mut ctx);
                let args = vec![ctx.null().upcast(), result.upcast()];
                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }
}

impl StateDB {
    pub fn js_range(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let option_inputs = ctx.argument::<JsObject>(1)?;
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let cb = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `SharedStateDB`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.range(cb, writer, options)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_upsert_key(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let key = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(2)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(3)?.root(&mut ctx);
        // Get the `this` value as a `SharedStateDB`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.upsert_key(cb, writer, key, value)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_delete_key(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let key = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `SharedStateDB`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.delete_key(cb, writer, key)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_get_key(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let key = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `SharedStateDB`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.get_key_with_writer(cb, writer, key)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_close(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        ctx.this()
            .downcast_or_throw::<SharedStateDB, _>(&mut ctx)?
            .borrow()
            .common
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
        db.common
            .get_by_key(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_get_current_state(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let cb = ctx.argument::<JsFunction>(0)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();
        db.get_current_state(cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.common
            .exists(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_revert(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let prev_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let height = ctx.argument::<JsNumber>(1)?.value(&mut ctx).into();
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
        db.common
            .send(move |conn, channel| {
                let iter = conn.iterator(utils::get_iteration_mode(&options, &mut vec![], true));
                for (counter, key_val) in iter.enumerate() {
                    if utils::is_key_out_of_range(
                        &options,
                        &(key_val.as_ref().unwrap().0),
                        counter as i64,
                        true,
                    ) {
                        break;
                    }
                    let c = Arc::clone(&a_cb_on_data);
                    channel.send(move |mut ctx| {
                        let (_, key_without_prefix) =
                            key_val.as_ref().unwrap().0.split_first().unwrap();
                        let temp_pair =
                            KVPair::new(key_without_prefix, &(key_val.as_ref().unwrap().1));
                        let obj = Self::pair_to_js_object(&mut ctx, &temp_pair)?;
                        let cb = c.lock().unwrap().to_inner(&mut ctx);
                        let this = ctx.undefined();
                        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), obj.upcast()];
                        cb.call(&mut ctx, this, args)?;
                        Ok(())
                    });
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
    // @params 1 version (required)
    // @params 2 prev_root (required)
    // @params 3 readonly
    // @params 4 expected_root
    // @params 5 check_root
    // @params 6 callback
    pub fn js_commit(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let writer = ctx.argument::<state_writer::SendableStateWriter>(0)?;

        let version = ctx.argument::<JsNumber>(1)?.value(&mut ctx).into();

        let prev_root = ctx.argument::<JsTypedArray<u8>>(2)?.as_slice(&ctx).to_vec();

        let readonly = ctx.argument::<JsBoolean>(3)?.value(&mut ctx);

        let expected = ctx.argument::<JsTypedArray<u8>>(4)?.as_slice(&ctx).to_vec();

        let check_root = ctx.argument::<JsBoolean>(5)?.value(&mut ctx);
        let cb = ctx.argument::<JsFunction>(6)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        if db.options.is_readonly() {
            return ctx.throw_error(String::from("Readonly DB cannot be committed."));
        }
        let options = CommitOptions::new(readonly, version);
        let commit = Commit::new(expected, options, check_root);
        let writer = Arc::clone(&writer.borrow());
        let commit_data = CommitData::new(commit, prev_root);
        db.commit(writer, commit_data, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_prove(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();

        let state_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();

        let input = ctx.argument::<JsArray>(1)?.to_vec(&mut ctx)?;
        let mut queries = NestedVec::new();
        for item in input.iter() {
            let obj = item.downcast_or_throw::<JsTypedArray<u8>, _>(&mut ctx)?;
            let key = obj.as_slice(&ctx).to_vec();
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
        let key_length = db.options.key_length();
        let state_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();

        let proof = Self::proof(&mut ctx)?;
        let parsed_query_keys = Self::parse_query_keys(&mut ctx)?;
        let cb = ctx.argument::<JsFunction>(3)?.root(&mut ctx);
        let channel = ctx.channel();

        thread::spawn(move || {
            let result =
                smt::SparseMerkleTree::verify(&parsed_query_keys, &proof, &state_root, key_length);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        vec![ctx.null().upcast(), JsBoolean::new(&mut ctx, val).upcast()]
                    },
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

        let version = ctx.argument::<JsNumber>(0)?.value(&mut ctx).into();

        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        db.clean_diff_until(version, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_checkpoint(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();

        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        db.common
            .checkpoint(path, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BlockHeight;

    #[test]
    fn test_current_state_convert() {
        let block_zero = BlockHeight(0);
        let block_ten = BlockHeight(10);
        let block_hundred = BlockHeight(100);
        let root = Vec::with_capacity(30);

        let test_data = vec![
            (
                CurrentState::new(&[], block_zero),
                [vec![], block_zero.to_be_bytes().to_vec()].concat(),
            ),
            (
                CurrentState::new(&[1, 2, 3, 4], block_ten),
                [vec![1, 2, 3, 4], block_ten.to_be_bytes().to_vec()].concat(),
            ),
            (
                CurrentState::new(&root, block_hundred),
                [root.clone(), block_hundred.to_be_bytes().to_vec()].concat(),
            ),
            (
                CurrentState::new(&EMPTY_HASH, block_zero),
                [EMPTY_HASH.to_vec(), block_zero.to_be_bytes().to_vec()].concat(),
            ),
        ];
        for (state_as_struct, state_as_bytes) in test_data {
            assert_eq!(state_as_struct.to_bytes(), state_as_bytes);
            assert_eq!(CurrentState::from_bytes(&state_as_bytes), state_as_struct);
        }
    }
}
