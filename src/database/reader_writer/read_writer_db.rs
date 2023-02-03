/// read_writer is the interface for state read writer.
/// State writer will snapshot the data and even if the change happen during the lifetime of reader writer, it will not be affected.
/// writer will not store the data to physical storage until commit to the state db.
use std::sync::{mpsc, Arc};

use neon::context::{Context, FunctionContext};
use neon::handle::Root;
use neon::object::Object;
use neon::result::JsResult;
use neon::types::{buffer::TypedArray, JsBuffer, JsFunction, JsObject, JsTypedArray, JsUndefined};

use crate::database::options;
use crate::database::reader_writer::{ReaderBase, SharedReaderBase};
use crate::database::types::{Kind, SnapshotMessage};
use crate::database::utils::*;
use crate::state_writer;
use crate::types::{ArcMutex, KVPair, SharedKVPair};

pub type ReadWriter = ReaderBase;
impl ReadWriter {
    /// update or insert the pair of key and value
    fn upsert_key(
        &self,
        callback: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        key: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<(), mpsc::SendError<SnapshotMessage>> {
        let state_db_key = Kind::State.key(key.clone());
        self.send(move |conn, channel| {
            let value = conn.get(&state_db_key);
            channel.send(move |mut ctx| {
                let args = {
                    let mut writer = writer.lock().unwrap();
                    let cached = writer.is_cached(&key);
                    if cached {
                        //  if the key already in cache so update it and returns
                        let result = writer.update(&KVPair::new(&key, &new_value));
                        parse_update_result(&mut ctx, result)?
                    } else if let Ok(value) = &value {
                        // if found the value of the key then insert into cache and update it
                        if value.is_some() {
                            let temp_value = value.as_ref().unwrap().to_vec();
                            let pair = SharedKVPair::new(&key, &temp_value);
                            writer.cache_existing(&pair);
                            let result = writer.update(&KVPair::new(&key, &new_value));
                            parse_update_result(&mut ctx, result)?
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
                let callback = callback.into_inner(&mut ctx);
                callback.call(&mut ctx, this, args)?;
                Ok(())
            });
        })
    }

    fn get_key_with_writer(
        &self,
        callback: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        key: Vec<u8>,
    ) -> Result<(), mpsc::SendError<SnapshotMessage>> {
        let state_db_key = Kind::State.key(key.clone());
        self.send(move |conn, channel| {
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
                let callback = callback.into_inner(&mut ctx);
                callback.call(&mut ctx, this, args)?;
                Ok(())
            });
        })
    }

    fn delete_key(
        &self,
        callback: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        key: Vec<u8>,
    ) -> Result<(), mpsc::SendError<SnapshotMessage>> {
        let state_db_key = Kind::State.key(key.clone());
        self.send(move |conn, channel| {
            let value = conn.get(&state_db_key);
            channel.send(move |mut ctx| {
                let this = ctx.undefined();
                let callback = callback.into_inner(&mut ctx);
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

    fn range(
        &self,
        callback: Root<JsFunction>,
        writer: ArcMutex<state_writer::StateWriter>,
        options: options::IterationOption,
    ) -> Result<(), mpsc::SendError<SnapshotMessage>> {
        self.send(move |conn, channel| {
            let values = conn
                .iterator(get_iteration_mode(&options, &mut vec![], true))
                .map(|key_val| {
                    KVPair::new(&key_val.as_ref().unwrap().0.clone(), &key_val.unwrap().1)
                })
                .collect::<Vec<KVPair>>();
            channel.send(move |mut ctx| {
                let result = {
                    let mut writer = writer.lock().unwrap();
                    let mut result = writer.get_range(&options);
                    for (counter, pair) in values.iter().enumerate() {
                        if is_key_out_of_range(&options, pair.key(), counter as i64, true) {
                            break;
                        }
                        let (_, key_without_prefix) = pair.key().split_first().unwrap();
                        let (cached_value, deleted, exists) = writer.get(key_without_prefix);
                        if exists && !deleted {
                            result.insert(key_without_prefix.to_vec(), cached_value);
                        } else if deleted {
                            continue;
                        } else {
                            // key is stored and returned without prefix
                            let shared_pair = SharedKVPair::new(key_without_prefix, pair.value());
                            writer.cache_existing(&shared_pair);
                            result.insert(shared_pair.key_as_vec(), shared_pair.value_as_vec());
                        }
                    }
                    cache_to_js_array(&mut ctx, &result)?
                };
                let this = ctx.undefined();
                let callback = callback.into_inner(&mut ctx);
                let args = vec![ctx.null().upcast(), result.upcast()];
                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    /// js_upsert_key is handler for JS ffi.
    /// it creates record if key does not exist, and if key exist, it will treat it as update.
    /// js "this" - ReadWriter.
    /// - @params(0) - StateWriter
    /// - @params(1) - key to set to the db.
    /// - @params(2) - value to set to the db.
    /// - @params(3) - callback to return the fetched value.
    /// - @callback(0) - Error
    pub fn js_upsert_key(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let key = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(2)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(3)?.root(&mut ctx);
        // Get the `this` value as a `SharedReaderBase`
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;
        let db = db.borrow();

        let writer = Arc::clone(&batch.borrow_mut());
        db.upsert_key(callback, writer, key, value)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_get is handler for JS ffi.
    /// js "this" - ReadWriter.
    /// - @params(0) - StateWriter
    /// - @params(1) - key to get from db.
    /// - @params(2) - callback to return the fetched value.
    /// - @callback(0) - Error. If data is not found, it will call the callback with "No data" as a first args.
    /// - @callback(1) - [u8]. Value associated with the key.
    pub fn js_get_key(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let key = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `SharedReaderBase`
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.get_key_with_writer(callback, writer, key)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_del is handler for JS ffi.
    /// js "this" - ReadWriter.
    /// - @params(0) - StateWriter
    /// - @params(1) - key to delete from the db.
    /// - @params(2) - callback to return the fetched value.
    /// - @callback(0) - Error
    pub fn js_delete_key(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let key = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `SharedReaderBase`
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.delete_key(callback, writer, key)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_range is handler for JS ffi.
    /// js "this" - ReadWriter.
    /// - @params(0) - StateWriter
    /// - @params(1) - Options for iteration. {limit: u32, reverse: bool, gte: &[u8], lte: &[u8]}.
    /// - @params(2) - Callback to be called on each data iteration.
    /// - @callback(0) - Error.
    /// - @callback(1) - [{ key: &[u8], value: &[u8]}].
    pub fn js_range(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the batch value as a `SendableStateWriter`
        let batch = ctx
            .argument::<state_writer::SendableStateWriter>(0)?
            .downcast_or_throw::<state_writer::SendableStateWriter, _>(&mut ctx)?;
        let option_inputs = ctx.argument::<JsObject>(1)?;
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let callback = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `SharedReaderBase`
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;
        let db = db.borrow_mut();
        let writer = Arc::clone(&batch.borrow_mut());
        db.range(callback, writer, options)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}
