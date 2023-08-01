use core::cell::RefCell;
/// state_db is an authenticated storage using Sparse Merkle Tree extending Database using rocksdb.
use std::cmp;
use std::convert::TryInto;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread;

use neon::prelude::*;
use neon::types::buffer::TypedArray;
use thiserror::Error;

use crate::batch;
use crate::consts;
use crate::database::options;
use crate::database::traits::{JsNewWithBoxRef, NewDBWithContext, OptionsWithContext, Unwrap};
use crate::database::types::{ArcOptionDB, DbMessage, DbOptions, JsBoxRef, Kind};
use crate::database::utils as DbUtils;
use crate::database::utils::pair_to_js_object;
use crate::database::DB;
use crate::diff;
use crate::sparse_merkle_tree::smt::{self, SMTError, EMPTY_HASH};
use crate::sparse_merkle_tree::smt_db;
use crate::state::state_writer;
use crate::types::{
    ArcMutex, BlockHeight, CommitOptions, KVPair, KeyLength, NestedVec, SharedVec,
};
use crate::utils;

pub type SharedStateDB = JsBoxRef<StateDB>;

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

/// StateDB maintains instance of database for authenticated storage using sparse merkle tree.
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
        kind: Kind,
    ) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
    {
        Ok(Self {
            common: DB::new_db_with_context(ctx, path, db_options, kind)?,
            options: db_options,
        })
    }
}

impl JsNewWithBoxRef for StateDB {
    fn js_new_with_box_ref<T: OptionsWithContext, U: NewDBWithContext + Send + Finalize>(
        mut ctx: FunctionContext,
    ) -> JsResult<JsBoxRef<U>> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let options = ctx.argument_opt(1);
        let db_opts = T::new_with_context(&mut ctx, options)?;
        let db = U::new_db_with_context(&mut ctx, path, db_opts, Kind::State)
            .or_else(|err| ctx.throw_error(&err))?;
        let ref_db = RefCell::new(db);

        return Ok(ctx.boxed(ref_db));
    }
}

impl Finalize for StateDB {}
impl StateDB {
    fn get_revert_result(
        conn: &DB,
        version: BlockHeight,
        state_root: &[u8],
        key_length: KeyLength,
    ) -> Result<SharedVec, DataStoreError> {
        let diff_bytes = conn
            .get(&[consts::Prefix::DIFF, &version.to_be_bytes()].concat())
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?
            .ok_or_else(|| DataStoreError::DiffNotFound(version.into()))?;

        let diff = diff::Diff::decode(&diff_bytes)
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?;
        let data = smt::UpdateData::new_from(diff.revert_hashed_update());
        let mut smt_db = smt_db::SmtDB::new(conn);
        let mut tree = smt::SparseMerkleTree::new(state_root, key_length, consts::SUBTREE_HEIGHT);
        let prev_root = tree
            .commit(&mut smt_db, &data)
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?;

        let mut write_batch = batch::PrefixWriteBatch::new();
        // Insert state batch with diff
        write_batch.set_prefix(&consts::Prefix::STATE);
        diff.revert_commit(&mut write_batch);
        write_batch.set_prefix(&consts::Prefix::DIFF);
        write_batch.delete(&version.to_be_bytes());

        // insert SMT batch
        write_batch.set_prefix(&consts::Prefix::SMT);
        smt_db.batch.iterate(&mut write_batch);
        // insert diff
        conn.write(write_batch.batch)
            .map_err(|err| DataStoreError::Unknown(err.to_string()))?;

        Ok(prev_root)
    }

    fn revert(
        &mut self,
        version: BlockHeight,
        state_root: Vec<u8>,
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let key_length = self.options.key_length();
        let result = StateDB::get_revert_result(&self.common, version, &state_root, key_length);
        if result.is_ok() {
            let value = (**result.as_ref().unwrap().lock().unwrap()).clone();
            let state_info = CurrentState::new(&value, version - BlockHeight(1));
            self.common
                .put(consts::Prefix::CURRENT_STATE, &state_info.to_bytes())
                .expect("Update state info should not be failed");
        }
        self.common.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
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
        conn: &DB,
        smt_db: &smt_db::SmtDB,
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
        smt_db.batch.iterate(&mut write_batch);
        // insert diff
        let result = conn.write(write_batch.batch);
        let version = info.data.options.version();
        match result {
            Ok(_) => {
                let value = (**root.as_ref().lock().unwrap()).clone();
                let state_info = CurrentState::new(&value, version);
                conn.put(consts::Prefix::CURRENT_STATE, &state_info.to_bytes())
                    .expect("Update state info should not be failed");
                Ok(root)
            },
            Err(err) => Err(smt::SMTError::Unknown(err.to_string())),
        }
    }

    fn commit(
        &mut self,
        writer: ArcMutex<state_writer::StateWriter>,
        commit_data: CommitData,
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let key_length = self.options.key_length();
        let w = writer.lock().unwrap();
        let data = smt::UpdateData::new_from(w.get_hashed_updated());
        let mut smt_db = smt_db::SmtDB::new(&self.common);
        let mut tree =
            smt::SparseMerkleTree::new(&commit_data.prev_root, key_length, consts::SUBTREE_HEIGHT);
        let root = tree.commit(&mut smt_db, &data);
        let result_info = CommitResultInfo::new(root, commit_data.data);
        let result = StateDB::handle_commit_result(&self.common, &smt_db, w, result_info);
        self.common.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
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
        callback: Root<JsFunction>,
    ) -> Result<(), DataStoreError> {
        let key_length = self.options.key_length();
        let mut tree = smt::SparseMerkleTree::new(&root, key_length, consts::SUBTREE_HEIGHT);
        let mut smtdb = smt_db::SmtDB::new(&self.common);
        let result = tree.prove(&mut smtdb, &queries);

        self.common
            .send(move |channel| {
                channel.send(move |mut ctx| {
                    let callback = callback.into_inner(&mut ctx);
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
                                let obj = pair_to_js_object(&mut ctx, &v.pair)?;
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
        callback: Root<JsFunction>,
    ) -> Result<(), DataStoreError> {
        if version.is_equal_to(0) {
            return Ok(());
        }
        let conn = self.common.arc_clone();
        self.common
            .send(move |channel| {
                let start = [consts::Prefix::DIFF, 0_u32.to_be_bytes().as_slice()].concat();
                let bytes = (version - BlockHeight(1)).to_be_bytes();
                let end = [consts::Prefix::DIFF, &bytes].concat();
                let mut batch = rocksdb::WriteBatch::default();

                let conn_iter = conn.unwrap().iterator(rocksdb::IteratorMode::From(
                    end.as_ref(),
                    rocksdb::Direction::Reverse,
                ));

                for key_val in conn_iter {
                    if utils::compare(&(key_val.as_ref().unwrap().0), &start)
                        == cmp::Ordering::Less
                    {
                        break;
                    }
                    batch.delete(&(key_val.unwrap().0));
                }

                let result = conn.unwrap().write(batch);

                channel.send(move |mut ctx| {
                    let callback = callback.into_inner(&mut ctx);
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

    fn proof(ctx: &mut FunctionContext, pos: u8) -> NeonResult<smt::Proof> {
        let raw_proof = ctx.argument::<JsObject>(pos.into())?;
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
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let result = self.common.get(consts::Prefix::CURRENT_STATE);
        self.common.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
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

    pub fn arc_clone(&self) -> ArcOptionDB {
        self.common.arc_clone()
    }
}

impl StateDB {
    /// js_close is handler for JS ffi.
    /// js "this" - StateDB.
    pub fn js_close(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        ctx.this()
            .downcast_or_throw::<SharedStateDB, _>(&mut ctx)?
            .borrow_mut()
            .common
            .close()
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_get is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - key to get from state db.
    /// - @params(1) - callback to return the fetched value.
    /// - @callback(0) - Error. If data is not found, it will call the callback with "No data" as a first args.
    /// - @callback(1) - [u8]. Value associated with the key.
    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.common
            .get_by_key(key, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_get_current_state is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - callback to return the fetched value.
    /// - @callback(0) - Error
    /// - @callback(1) - { root: [u8], version: u32 }.
    pub fn js_get_current_state(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let callback = ctx.argument::<JsFunction>(0)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();
        db.get_current_state(callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_exists is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - key to check existence from state db.
    /// - @params(1) - callback to return the fetched value.
    /// - @callback(0) - Error
    /// - @callback(1) - bool
    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.common
            .exists(key, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_revert is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - State root of to revert back from.
    /// - @params(1) - Version of the state DB to revert back from.
    /// - @params(2) - callback to return the result.
    /// - @callback(0) - Error.
    /// - @callback(1) - &[u8] State root after the revert.
    pub fn js_revert(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let prev_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let height = ctx.argument::<JsNumber>(1)?.value(&mut ctx).into();
        let callback = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;

        let mut db = db.borrow_mut();
        db.revert(height, prev_root, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_iterate is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - Options for iteration. {limit: u32, reverse: bool, gte: &[u8], lte: &[u8]}.
    /// - @params(1) - Callback to be called on each data iteration.
    /// - @params(2) - callback to be called when completing the iteration.
    /// - @callback1(0) - Error.
    /// - @callback1(1) - { key: &[u8], value: &[u8]}.
    /// - @callback(0) - void.
    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let callback_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let callback_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow_mut();

        let callback_on_data = Arc::new(Mutex::new(callback_on_data));
        let conn = db.common.arc_clone();
        db.common
            .send(move |channel| {
                let conn_iter = conn.unwrap().iterator(DbUtils::get_iteration_mode(
                    &options,
                    &mut vec![],
                    true,
                ));
                for (counter, key_val) in conn_iter.enumerate() {
                    if DbUtils::is_key_out_of_range(
                        &options,
                        &(key_val.as_ref().unwrap().0),
                        counter as i64,
                        true,
                    ) {
                        break;
                    }
                    let callback_on_data = Arc::clone(&callback_on_data);
                    channel.send(move |mut ctx| {
                        let (_, key_without_prefix) =
                            key_val.as_ref().unwrap().0.split_first().unwrap();
                        let temp_pair =
                            KVPair::new(key_without_prefix, &(key_val.as_ref().unwrap().1));
                        let obj = pair_to_js_object(&mut ctx, &temp_pair)?;
                        let callback = callback_on_data.lock().unwrap().to_inner(&mut ctx);
                        let this = ctx.undefined();
                        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), obj.upcast()];
                        callback.call(&mut ctx, this, args)?;
                        Ok(())
                    });
                }
                channel.send(move |mut ctx| {
                    let callback_done = callback_done.into_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast()];
                    callback_done.call(&mut ctx, this, args)?;

                    Ok(())
                });
            })
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_commit is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - writer instance (required).
    /// - @params(1) - version of current state_db (required).
    /// - @params(2) - current state root (required).
    /// - @params(3) - readonly not update the state to the physical storage.
    /// - @params(4) - expected state root to compare.
    /// - @params(5) - whether to check the root before storing to the physical storage.
    /// - @params(6) - callback to return the result.
    /// - @callback(0) - Error.
    /// - @callback(1) - &[u8] State root after the commit.
    pub fn js_commit(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let writer = ctx.argument::<state_writer::SendableStateWriter>(0)?;

        let version = ctx.argument::<JsNumber>(1)?.value(&mut ctx).into();

        let prev_root = ctx.argument::<JsTypedArray<u8>>(2)?.as_slice(&ctx).to_vec();

        let readonly = ctx.argument::<JsBoolean>(3)?.value(&mut ctx);

        let expected = ctx.argument::<JsTypedArray<u8>>(4)?.as_slice(&ctx).to_vec();

        let check_root = ctx.argument::<JsBoolean>(5)?.value(&mut ctx);
        let callback = ctx.argument::<JsFunction>(6)?.root(&mut ctx);
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
        db.commit(writer, commit_data, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_prove is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - current state root (required).
    /// - @params(1) - queries in format of &[&[u8]]
    /// - @params(2) - callback to return the result.
    /// - @callback(0) - Error.
    /// - @callback(1) - { siblingHashes: &[&[u8]]; queries: { key: &[u8]; value: &[u8]; bitmap: &[u8]; }[]; }
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

        let callback = ctx.argument::<JsFunction>(2)?.root(&mut ctx);

        db.prove(state_root, queries, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_verify is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - current state root.
    /// - @params(1) - queries in format of &[&[u8]]
    /// - @params(2) - proof { siblingHashes: &[&[u8]]; queries: { key: &[u8]; value: &[u8]; bitmap: &[u8]; }[]; }
    /// - @params(3) - callback to return the result.
    /// - @callback(0) - Error.
    /// - @callback(1) - bool represents true if proof is valid.
    pub fn js_verify(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();
        let key_length = db.options.key_length();
        let state_root = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();

        let proof = Self::proof(&mut ctx, 2)?;
        let parsed_query_keys = Self::parse_query_keys(&mut ctx)?;
        let callback = ctx.argument::<JsFunction>(3)?.root(&mut ctx);
        let channel = ctx.channel();

        thread::spawn(move || {
            let result =
                smt::SparseMerkleTree::verify(&parsed_query_keys, &proof, &state_root, key_length);

            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
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

    /// js_clean_diff_until is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - version to delete state diff upto.
    /// - @params(1) - callback to return the result.
    /// - @callback(0) - Error.
    pub fn js_clean_diff_until(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();

        let version = ctx.argument::<JsNumber>(0)?.value(&mut ctx).into();

        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        db.clean_diff_until(version, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_checkpoint is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - path to create the checkpoint.
    /// - @params(1) - callback to return the result.
    /// - @callback(0) - Error.
    pub fn js_checkpoint(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let db = ctx.this().downcast_or_throw::<SharedStateDB, _>(&mut ctx)?;
        let db = db.borrow();

        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        db.common
            .checkpoint(path, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_calculate_root is handler for JS ffi.
    /// js "this" - StateDB.
    /// - @params(0) - proof { siblingHashes: &[&[u8]]; queries: { key: &[u8]; value: &[u8]; bitmap: &[u8]; }[]; }
    /// - @params(1) - callback to return the result.
    /// - @callback(0) - Error.
    /// - @callback(1) - root: &[u8].
    pub fn js_calculate_root(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let proof = Self::proof(&mut ctx, 0)?;
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let channel = ctx.channel();

        thread::spawn(move || {
            let result: Result<Vec<u8>, SMTError> =
                match smt::SparseMerkleTree::prepare_queries_with_proof_map(&proof) {
                    Ok(filter_map) => {
                        let mut filtered_proof = filter_map
                            .values()
                            .cloned()
                            .collect::<Vec<smt::QueryProofWithProof>>();
                        smt::SparseMerkleTree::calculate_root(
                            &proof.sibling_hashes,
                            &mut filtered_proof,
                        )
                    },
                    Err(err) => Err(err),
                };

            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        vec![
                            ctx.null().upcast(),
                            JsBuffer::external(&mut ctx, val).upcast(),
                        ]
                    },
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };
                callback.call(&mut ctx, this, args)?;

                Ok(())
            })
        });

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
