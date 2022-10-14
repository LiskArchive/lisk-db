use std::sync::{mpsc, Arc, Mutex};

use neon::context::{Context, FunctionContext};
use neon::handle::{Handle, Root};
use neon::object::Object;
use neon::result::JsResult;
use neon::types::buffer::TypedArray;
use neon::types::{JsBoolean, JsFunction, JsObject, JsTypedArray, JsUndefined, JsValue};

use crate::db::options::IterationOption;
use crate::db::types::{JsBoxRef, Kind, SnapshotMessage};
use crate::db::utils::*;
use crate::db::ReaderBase;
use crate::types::KVPair;

pub type Reader = ReaderBase;
pub type SharedReaderDB = JsBoxRef<Reader>;

impl Reader {
    fn exists(
        &self,
        key: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<SnapshotMessage>> {
        let key = Kind::State.key(key);
        self.send(move |conn, channel| {
            let result = conn.get(&key);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
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

    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.get_by_key(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderDB, _>(&mut ctx)?;

        let db = db.borrow_mut();
        db.exists(key, cb)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let options = IterationOption::new(&mut ctx, option_inputs);
        let cb_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let cb_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<SharedReaderDB, _>(&mut ctx)?;
        let db = db.borrow();

        let a_cb_on_data = Arc::new(Mutex::new(cb_on_data));
        db.send(move |conn, channel| {
            let iter = conn.iterator(get_iteration_mode(&options, &mut vec![], true));
            for (counter, key_val) in iter.enumerate() {
                if is_key_out_of_range(
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
                    let obj = pair_to_js_object(&mut ctx, &temp_pair)?;
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
}
