// in_memory_smt provides in memory SMT computation without a physical storage.
use std::sync::Arc;
use std::thread;

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::consts;
use crate::database::traits::{DatabaseKind, JsNewWithArcMutex, NewDBWithKeyLength};
use crate::database::types::{JsArcMutex, Kind as DBKind};
use crate::sparse_merkle_tree::smt::{QueryProofWithProof, SMTError};
use crate::sparse_merkle_tree::smt_db;
use crate::sparse_merkle_tree::{Proof, QueryProof, SparseMerkleTree, UpdateData};
use crate::types::{ArcMutex, Cache, KVPair, KeyLength, NestedVec};

type SharedInMemorySMT = JsArcMutex<InMemorySMT>;
type DatabaseParameters = (ArcMutex<InMemorySMT>, Vec<u8>, Root<JsFunction>);
type VerifyParameters = (Vec<u8>, NestedVec, Proof, KeyLength, Root<JsFunction>);

struct JsFunctionContext<'a> {
    context: FunctionContext<'a>,
}

pub struct InMemorySMT {
    db: smt_db::InMemorySmtDB,
    key_length: KeyLength,
}

impl NewDBWithKeyLength for InMemorySMT {
    fn new_db_with_key_length(len: Option<KeyLength>) -> Self {
        Self {
            db: smt_db::InMemorySmtDB::default(),
            key_length: len.expect("The key_length should have a value"),
        }
    }
}

impl DatabaseKind for InMemorySMT {
    fn db_kind() -> DBKind {
        DBKind::InMemorySMT
    }
}

impl JsNewWithArcMutex for InMemorySMT {}
impl Finalize for InMemorySMT {}

impl JsFunctionContext<'_> {
    fn get_database_parameters(&mut self) -> NeonResult<DatabaseParameters> {
        let in_memory_smt = self
            .context
            .this()
            .downcast_or_throw::<SharedInMemorySMT, _>(&mut self.context)?;
        let in_memory_smt = Arc::clone(&in_memory_smt.borrow());

        let state_root = self
            .context
            .argument::<JsTypedArray<u8>>(0)?
            .as_slice(&self.context)
            .to_vec();
        let callback = self
            .context
            .argument::<JsFunction>(2)?
            .root(&mut self.context);

        Ok((in_memory_smt, state_root, callback))
    }

    fn get_key_value_pairs(&mut self) -> NeonResult<Cache> {
        let input = self
            .context
            .argument::<JsArray>(1)?
            .to_vec(&mut self.context)?;

        let mut data = Cache::new();
        for key in input.iter() {
            let obj = key.downcast_or_throw::<JsObject, _>(&mut self.context)?;
            let key = obj
                .get::<JsTypedArray<u8>, _, _>(&mut self.context, "key")?
                .as_slice(&self.context)
                .to_vec();
            let value = obj
                .get::<JsTypedArray<u8>, _, _>(&mut self.context, "value")?
                .as_slice(&self.context)
                .to_vec();
            data.insert(key, value);
        }
        Ok(data)
    }

    fn update_database(&mut self, data: Cache) -> NeonResult<()> {
        let (in_memory_smt, state_root, callback) = self.get_database_parameters()?;
        let channel = self.context.channel();

        thread::spawn(move || {
            let update_data = UpdateData::new_from(data);
            let mut inner_smt = in_memory_smt.lock().unwrap();

            let mut tree =
                SparseMerkleTree::new(&state_root, inner_smt.key_length, consts::SUBTREE_HEIGHT);

            let result = tree.commit(&mut inner_smt.db, &update_data);

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
            })
        });

        Ok(())
    }

    fn get_keys(&mut self) -> NeonResult<NestedVec> {
        let input = self
            .context
            .argument::<JsArray>(1)?
            .to_vec(&mut self.context)?;
        let mut data = NestedVec::new();
        for key in input.iter() {
            let key = key
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut self.context)?
                .as_slice(&self.context)
                .to_vec();
            data.push(key);
        }

        Ok(data)
    }

    fn prove(&mut self, data: NestedVec) -> NeonResult<()> {
        let (in_memory_smt, state_root, callback) = self.get_database_parameters()?;
        let channel = self.context.channel();

        thread::spawn(move || {
            let mut inner_smt = in_memory_smt.lock().unwrap();
            let mut tree =
                SparseMerkleTree::new(&state_root, inner_smt.key_length, consts::SUBTREE_HEIGHT);

            let result = tree.prove(&mut inner_smt.db, &data);

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
                        obj.set(&mut ctx, "queries", queries)?;
                        for (i, v) in val.queries.iter().enumerate() {
                            let obj = ctx.empty_object();
                            let key = JsBuffer::external(&mut ctx, v.key_as_vec());
                            obj.set(&mut ctx, "key", key)?;
                            let value = JsBuffer::external(&mut ctx, v.value_as_vec());
                            obj.set(&mut ctx, "value", value)?;
                            let bitmap = JsBuffer::external(&mut ctx, v.bitmap.to_vec());
                            obj.set(&mut ctx, "bitmap", bitmap)?;

                            queries.set(&mut ctx, i as u32, obj)?;
                        }
                        vec![ctx.null().upcast(), obj.upcast()]
                    },
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };
                callback.call(&mut ctx, this, args)?;

                Ok(())
            })
        });

        Ok(())
    }

    fn get_proof(&mut self, pos: u8) -> NeonResult<Proof> {
        let raw_proof = self.context.argument::<JsObject>(pos.into())?;
        let mut sibling_hashes = NestedVec::new();
        let raw_sibling_hashes = raw_proof
            .get::<JsArray, _, _>(&mut self.context, "siblingHashes")?
            .to_vec(&mut self.context)?;
        for raw_sibling_hash in raw_sibling_hashes.iter() {
            let sibling_hash = raw_sibling_hash
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut self.context)?
                .as_slice(&self.context)
                .to_vec();
            sibling_hashes.push(sibling_hash);
        }
        let raw_queries = raw_proof
            .get::<JsArray, _, _>(&mut self.context, "queries")?
            .to_vec(&mut self.context)?;
        let mut queries: Vec<QueryProof> = Vec::with_capacity(raw_queries.len());
        for key in raw_queries.iter() {
            let obj = key.downcast_or_throw::<JsObject, _>(&mut self.context)?;
            let key = obj
                .get::<JsTypedArray<u8>, _, _>(&mut self.context, "key")?
                .as_slice(&self.context)
                .to_vec();
            let value = obj
                .get::<JsTypedArray<u8>, _, _>(&mut self.context, "value")?
                .as_slice(&self.context)
                .to_vec();
            let bitmap = obj
                .get::<JsTypedArray<u8>, _, _>(&mut self.context, "bitmap")?
                .as_slice(&self.context)
                .to_vec();
            queries.push(QueryProof {
                pair: Arc::new(KVPair::new(&key, &value)),
                bitmap: Arc::new(bitmap),
            });
        }
        let proof = Proof {
            queries,
            sibling_hashes,
        };

        Ok(proof)
    }

    fn get_verify_parameters(&mut self) -> NeonResult<VerifyParameters> {
        let state_root = self
            .context
            .argument::<JsTypedArray<u8>>(0)?
            .as_slice(&self.context)
            .to_vec();

        let query_keys = self
            .context
            .argument::<JsArray>(1)?
            .to_vec(&mut self.context)?;
        let mut parsed_query_keys = NestedVec::new();
        for key in query_keys.iter() {
            let key = key
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut self.context)?
                .as_slice(&self.context)
                .to_vec();
            parsed_query_keys.push(key);
        }

        let proof = self.get_proof(2)?;

        let key_length = self
            .context
            .argument::<JsNumber>(3)?
            .value(&mut self.context)
            .into();
        let callback = self
            .context
            .argument::<JsFunction>(4)?
            .root(&mut self.context);

        Ok((state_root, parsed_query_keys, proof, key_length, callback))
    }
}

impl InMemorySMT {
    /// js_update is handler for JS ffi.
    /// it is the similar to StateDB commit, but it uses in memory database.
    pub fn js_update(ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut js_context = JsFunctionContext { context: ctx };

        let data = js_context.get_key_value_pairs()?;
        js_context.update_database(data)?;

        Ok(js_context.context.undefined())
    }

    /// js_prove is handler for JS ffi.
    /// it is the similar to StateDB prove, but it uses in memory database.
    pub fn js_prove(ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut js_context = JsFunctionContext { context: ctx };

        let data = js_context.get_keys()?;
        js_context.prove(data)?;

        Ok(js_context.context.undefined())
    }

    /// js_verify is handler for JS ffi.
    /// it is the similar to StateDB verify, but it uses in memory database.
    pub fn js_verify(ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut js_context = JsFunctionContext { context: ctx };

        let (state_root, parsed_query_keys, proof, key_length, callback) =
            js_context.get_verify_parameters()?;
        let channel = js_context.context.channel();

        thread::spawn(move || {
            let result =
                SparseMerkleTree::verify(&parsed_query_keys, &proof, &state_root, key_length);

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

        Ok(js_context.context.undefined())
    }

    /// js_calculate_root is handler for JS ffi.
    /// it calculate and returns the root hash of the in memory database.
    pub fn js_calculate_root(ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut js_context = JsFunctionContext { context: ctx };

        let proof = js_context.get_proof(0)?;
        let callback = js_context
            .context
            .argument::<JsFunction>(1)?
            .root(&mut js_context.context);
        let channel = js_context.context.channel();

        thread::spawn(move || {
            let result: Result<Vec<u8>, SMTError> =
                match SparseMerkleTree::prepare_queries_with_proof_map(&proof) {
                    Ok(filter_map) => {
                        let mut filtered_proof = filter_map
                            .values()
                            .cloned()
                            .collect::<Vec<QueryProofWithProof>>();
                        SparseMerkleTree::calculate_root(
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

        Ok(js_context.context.undefined())
    }
}
