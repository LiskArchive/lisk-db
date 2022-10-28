use std::sync::{Arc, Mutex};

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::batch;
use crate::database::options::IterationOption;
use crate::database::traits::JsNewWithBox;
use crate::database::utils;
use crate::database::DB;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

pub type Database = DB;
impl JsNewWithBox for Database {}
impl Database {
    fn send_over_channel(
        channel: &Channel,
        callback: Root<JsFunction>,
        result: Result<(), rocksdb::Error>,
    ) {
        channel.send(move |mut ctx| {
            let callback = callback.into_inner(&mut ctx);
            let this = ctx.undefined();
            let args: Vec<Handle<JsValue>> = match result {
                Ok(_) => vec![ctx.null().upcast()],
                Err(err) => vec![ctx.error(&err)?.upcast()],
            };

            callback.call(&mut ctx, this, args)?;

            Ok(())
        });
    }

    pub fn js_clear(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let conn = db.arc_clone();
        db.send(move |channel| {
            let mut batch = rocksdb::WriteBatch::default();
            let conn_iter = conn.iterator(rocksdb::IteratorMode::Start);
            for key_val in conn_iter {
                batch.delete(&(key_val.unwrap().0));
            }
            let result = conn.write(batch);
            Database::send_over_channel(channel, callback, result);
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_close(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        ctx.this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?
            .close()
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.get_by_key(key, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.exists(key, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let result = db.put(&key, &value);
        db.send(move |channel| {
            Database::send_over_channel(channel, callback, result);
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let result = db.delete(&key);
        db.send(move |channel| {
            Database::send_over_channel(channel, callback, result);
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_write(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let batch = ctx
            .argument::<batch::SendableWriteBatch>(0)?
            .downcast_or_throw::<batch::SendableWriteBatch, _>(&mut ctx)?;
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let batch = Arc::clone(&batch.borrow());
        let conn = db.arc_clone();
        db.send(move |channel| {
            let write_batch = rocksdb::WriteBatch::default();
            let inner_batch = batch.lock().unwrap();
            let mut write_batch = batch::WriteBatch { batch: write_batch };
            inner_batch.batch.iterate(&mut write_batch);
            let result = conn.write(write_batch.batch);
            Database::send_over_channel(channel, callback, result);
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = IterationOption::new(&mut ctx, option_inputs);
        let callback_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let callback_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let callback_on_data = Arc::new(Mutex::new(callback_on_data));
        let conn = db.arc_clone();
        db.send(move |channel| {
            let iter = conn.iterator(utils::get_iteration_mode(&options, &mut vec![], false));
            for (counter, key_val) in iter.enumerate() {
                if utils::is_key_out_of_range(
                    &options,
                    &(key_val.as_ref().unwrap().0),
                    counter as i64,
                    false,
                ) {
                    break;
                }
                let callback_on_data = Arc::clone(&callback_on_data);
                channel.send(move |mut ctx| {
                    let obj = ctx.empty_object();
                    let key_res =
                        JsBuffer::external(&mut ctx, key_val.as_ref().unwrap().0.clone());
                    let val_res = JsBuffer::external(&mut ctx, key_val.unwrap().1);
                    obj.set(&mut ctx, "key", key_res)?;
                    obj.set(&mut ctx, "value", val_res)?;
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

    pub fn js_checkpoint(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.checkpoint(path, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}
