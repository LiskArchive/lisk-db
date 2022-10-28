use std::sync::{Arc, Mutex};

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::batch;
use crate::db::options::IterationOption;
use crate::db::traits::JsNewWithBox;
use crate::db::utils;
use crate::db::DB;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

pub type Database = DB;
impl JsNewWithBox for Database {}
impl Database {
    pub fn js_clear(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let conn = db.arc_clone();
        db.send(move |channel| {
            let mut batch = rocksdb::WriteBatch::default();
            let iter = conn.iterator(rocksdb::IteratorMode::Start);
            for key_val in iter {
                batch.delete(&(key_val.unwrap().0));
            }
            let result = conn.write(batch);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
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
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.get_by_key(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.exists(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let value = ctx.argument::<JsTypedArray<u8>>(1)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let result = db.put(&key, &value);
        db.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_del(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let result = db.delete(&key);
        db.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_write(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let batch = ctx
            .argument::<batch::SendableWriteBatch>(0)?
            .downcast_or_throw::<batch::SendableWriteBatch, _>(&mut ctx)?;
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let batch = Arc::clone(&batch.borrow());
        let conn = db.arc_clone();
        db.send(move |channel| {
            let b = rocksdb::WriteBatch::default();
            let inner_batch = batch.lock().unwrap();
            let mut write_batch = batch::WriteBatch { batch: b };
            inner_batch.batch.iterate(&mut write_batch);
            let result = conn.write(write_batch.batch);
            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = IterationOption::new(&mut ctx, option_inputs);
        let cb_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let cb_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let a_cb_on_data = Arc::new(Mutex::new(cb_on_data));
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
                let c = Arc::clone(&a_cb_on_data);
                channel.send(move |mut ctx| {
                    let obj = ctx.empty_object();
                    let key_res =
                        JsBuffer::external(&mut ctx, key_val.as_ref().unwrap().0.clone());
                    let val_res = JsBuffer::external(&mut ctx, key_val.unwrap().1);
                    obj.set(&mut ctx, "key", key_res)?;
                    obj.set(&mut ctx, "value", val_res)?;
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

    pub fn js_checkpoint(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.checkpoint(path, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}
