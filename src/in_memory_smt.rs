use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::consts;
use crate::smt::{Proof, QueryProof, SparseMerkleTree, UpdateData};
use crate::smt_db;

type SharedInMemorySMT = JsBox<RefCell<Arc<Mutex<InMemorySMT>>>>;
type DatabaseParameters = (Arc<Mutex<InMemorySMT>>, Vec<u8>, Root<JsFunction>);
type VerifyParameters = (Vec<u8>, Vec<Vec<u8>>, Proof, usize, Root<JsFunction>);

struct JsFunctionContext<'a> {
    context: FunctionContext<'a>,
}

pub struct InMemorySMT {
    db: smt_db::InMemorySmtDB,
    key_length: usize,
}

impl JsFunctionContext<'_> {
    fn get_database_parameters(&mut self) -> NeonResult<DatabaseParameters> {
        let in_memory_smt = self
            .context
            .this()
            .downcast_or_throw::<SharedInMemorySMT, _>(&mut self.context)?;
        let in_memory_smt = in_memory_smt.borrow().clone();

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

    fn get_key_value_pairs(&mut self) -> NeonResult<HashMap<Vec<u8>, Vec<u8>>> {
        let input = self
            .context
            .argument::<JsArray>(1)?
            .to_vec(&mut self.context)?;

        let mut data: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
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

    fn update_database(&mut self, data: HashMap<Vec<u8>, Vec<u8>>) -> NeonResult<()> {
        let (in_memory_smt, state_root, callback) = self.get_database_parameters()?;
        let channel = self.context.channel();

        thread::spawn(move || {
            let mut update_data = UpdateData::new_from(data);
            let mut inner_smt = in_memory_smt.lock().unwrap();

            let mut tree =
                SparseMerkleTree::new(state_root, inner_smt.key_length, consts::SUBTREE_SIZE);

            let result = tree.commit(&mut inner_smt.db, &mut update_data);

            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let buffer = JsBuffer::external(&mut ctx, val.to_vec());
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

    fn get_keys(&mut self) -> NeonResult<Vec<Vec<u8>>> {
        let input = self
            .context
            .argument::<JsArray>(1)?
            .to_vec(&mut self.context)?;
        let mut data: Vec<Vec<u8>> = vec![];
        for key in input.iter() {
            let key = key
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut self.context)?
                .as_slice(&self.context)
                .to_vec();
            data.push(key);
        }

        Ok(data)
    }

    fn prove(&mut self, data: Vec<Vec<u8>>) -> NeonResult<()> {
        let (in_memory_smt, state_root, callback) = self.get_database_parameters()?;
        let channel = self.context.channel();

        thread::spawn(move || {
            let mut inner_smt = in_memory_smt.lock().unwrap();
            let mut tree =
                SparseMerkleTree::new(state_root, inner_smt.key_length, consts::SUBTREE_SIZE);

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
                            let key = JsBuffer::external(&mut ctx, v.key.to_vec());
                            obj.set(&mut ctx, "key", key)?;
                            let value = JsBuffer::external(&mut ctx, v.value.to_vec());
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

    fn get_proof(&mut self) -> NeonResult<Proof> {
        let raw_proof = self.context.argument::<JsObject>(2)?;
        let mut sibling_hashes: Vec<Vec<u8>> = vec![];
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
        let mut queries: Vec<QueryProof> = vec![];
        let raw_queries = raw_proof
            .get::<JsArray, _, _>(&mut self.context, "queries")?
            .to_vec(&mut self.context)?;
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
            queries.push(QueryProof { key, value, bitmap });
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
        let mut parsed_query_keys: Vec<Vec<u8>> = vec![];
        for key in query_keys.iter() {
            let key = key
                .downcast_or_throw::<JsTypedArray<u8>, _>(&mut self.context)?
                .as_slice(&self.context)
                .to_vec();
            parsed_query_keys.push(key);
        }

        let proof = self.get_proof()?;

        let key_length = self
            .context
            .argument::<JsNumber>(3)?
            .value(&mut self.context) as usize;
        let callback = self
            .context
            .argument::<JsFunction>(4)?
            .root(&mut self.context);

        Ok((state_root, parsed_query_keys, proof, key_length, callback))
    }
}

impl Finalize for InMemorySMT {}

impl InMemorySMT {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<SharedInMemorySMT> {
        let key_length = ctx.argument::<JsNumber>(0)?.value(&mut ctx) as usize;
        let tree = InMemorySMT {
            db: smt_db::InMemorySmtDB::new(),
            key_length,
        };

        let ref_tree = RefCell::new(Arc::new(Mutex::new(tree)));
        return Ok(ctx.boxed(ref_tree));
    }

    pub fn js_update(ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut js_context = JsFunctionContext { context: ctx };

        let data = js_context.get_key_value_pairs()?;
        js_context.update_database(data)?;

        Ok(js_context.context.undefined())
    }

    pub fn js_prove(ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut js_context = JsFunctionContext { context: ctx };

        let data = js_context.get_keys()?;
        js_context.prove(data)?;

        Ok(js_context.context.undefined())
    }

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
}
