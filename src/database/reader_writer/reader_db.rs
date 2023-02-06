/// reader_db is the interface for state reader.
/// State reader will snapshot the data and even if the change happen during the lifetime of reader, it will not be affected.
use std::sync::{mpsc, Arc, Mutex};

use neon::context::{Context, FunctionContext};
use neon::handle::{Handle, Root};
use neon::object::Object;
use neon::result::JsResult;
use neon::types::buffer::TypedArray;
use neon::types::{JsBoolean, JsFunction, JsObject, JsTypedArray, JsUndefined, JsValue};

use crate::database::options::IterationOption;
use crate::database::reader_writer::{ReaderBase, SharedReaderBase};
use crate::database::types::{Kind, SnapshotMessage};
use crate::database::utils::*;
use crate::types::KVPair;

pub type Reader = ReaderBase;
impl Reader {
    fn exists(
        &self,
        key: Vec<u8>,
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<SnapshotMessage>> {
        let key = Kind::State.key(key);
        self.send(move |conn, channel| {
            let result = conn.get(&key);

            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(Some(_)) => {
                        vec![ctx.null().upcast(), JsBoolean::new(&mut ctx, true).upcast()]
                    },
                    Ok(None) => vec![
                        ctx.null().upcast(),
                        JsBoolean::new(&mut ctx, false).upcast(),
                    ],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    /// js_get is handler for JS ffi.
    /// js "this" - Reader.
    /// - @params(0) - key to get from db.
    /// - @params(1) - callback to return the fetched value.
    /// - @callback(0) - Error. If data is not found, it will call the callback with "No data" as a first args.
    /// - @callback(1) - [u8]. Value associated with the key.
    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.get_by_key(key, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_exists is handler for JS ffi.
    /// js "this" - Reader.
    /// - @params(0) - key to check existence from db.
    /// - @params(1) - callback to return the fetched value.
    /// - @callback(0) - Error
    /// - @callback(1) - bool
    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.exists(key, callback)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    /// js_iterate is handler for JS ffi.
    /// js "this" - Reader.
    /// - @params(0) - Options for iteration. {limit: u32, reverse: bool, gte: &[u8], lte: &[u8]}.
    /// - @params(1) - Callback to be called on each data iteration.
    /// - @params(2) - callback to be called when completing the iteration.
    /// - @callback1(0) - Error.
    /// - @callback1(1) - { key: &[u8], value: &[u8]}.
    /// - @callback(0) - void.
    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = IterationOption::new(&mut ctx, option_inputs);
        let callback_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let callback_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderBase, _>(&mut ctx)?;
        let db = db.borrow();

        let callback_on_data = Arc::new(Mutex::new(callback_on_data));
        db.send(move |conn, channel| {
            let conn_iter = conn.iterator(get_iteration_mode(&options, &mut vec![], true));
            for (counter, key_val) in conn_iter.enumerate() {
                if is_key_out_of_range(
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
}
