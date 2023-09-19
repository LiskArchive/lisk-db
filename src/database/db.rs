use std::sync::Mutex;
use std::sync::{Arc};
use std::thread;

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::database::options::IterationOption;
use crate::database::utils;
use crate::database::DB;
use crate::database::types::{DbOptions, Kind};

use super::db_base::{DBError};
use super::traits::OptionsWithContext;

struct WaitGroup {
    counter: Mutex<usize>,
}

impl WaitGroup {
    fn new() -> Self {
        WaitGroup {
            counter: Mutex::new(0),
        }
    }

    fn add(&self, delta: usize) {
        let mut count = self.counter.lock().unwrap();
        *count += delta;
    }

    fn done(&self) {
        let mut count = self.counter.lock().unwrap();
        if *count > 0 {
            *count -= 1;
        }
    }

    fn wait(&self) {
        loop {
            let count = *self.counter.lock().unwrap();
            if count == 0 {
                break;
            }
            // Sleep or yield to avoid busy-waiting
            thread::yield_now();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
}

#[derive(Clone)]
struct CallbackWrapper(Arc<Root<JsFunction>>);

impl Finalize for CallbackWrapper {
    fn finalize<'a, C: Context<'a>>(self, ctx: &mut C) {
        println!("finalize callback wrapper");
        self.0.finalize(ctx)

    }
}

impl CallbackWrapper {
    fn into_inner<'a, C: Context<'a>>(self, ctx: &mut C) -> Handle<'a, JsFunction> {
        // Ensure that if this is the last reference, the `Root` is dropped
        match Arc::try_unwrap(self.0) {
            Ok(v) => v.into_inner(ctx),
            Err(v) => v.as_ref().to_inner(ctx),
        }
    }
}

struct StreamCallback {
    channel: Arc<Channel>,
    callback: CallbackWrapper,
}

impl Finalize for StreamCallback {
    fn finalize<'a, C: Context<'a>>(self, ctx: &mut C) {
        println!("finalize stream callback");
        self.callback.finalize(ctx);
    }
}

impl StreamCallback {
    fn invoke<'a, C: Context<'a>>(self, ctx: &mut C, cdb: Arc<Mutex<rocksdb::DB>>, options: IterationOption) -> JsResult<'a, JsPromise>{
        let promise = ctx.task(move || {
            let locked = cdb.try_lock().map_err(|_| DBError::LockError)?;
            let iter =
                locked
                    .iterator(utils::get_iteration_mode(&options, &mut vec![], false));

                let mut i: usize = 0;
                let wg = Arc::new(WaitGroup::new());
                
                for (counter, key_val) in iter.enumerate() {
                    if utils::is_key_out_of_range(
                        &options,
                        &(key_val.as_ref().unwrap().0),
                        counter as i64,
                        false,
                    ) {
                        break;
                    }
                    // println!("{}", counter);
                    i += 1;
                    wg.add(1);
                    let callback_on_data = self.callback.clone();
                    let result = key_val.unwrap();
                    let _key = result.0.into_vec();
                    let _value = result.1.into_vec();
                    let channel = self.channel.clone();
                    let wg_clone = Arc::clone(&wg);
                    channel.send(move |mut ctx| {
                        let obj = ctx.empty_object();
                        let key_res =
                            JsBuffer::external(&mut ctx, _key);
                        let val_res = JsBuffer::external(&mut ctx, _value);
                        obj.set(&mut ctx, "key", key_res)?;
                        obj.set(&mut ctx, "value", val_res)?;
                        let callback = callback_on_data.into_inner(&mut ctx);
                        let this = ctx.undefined();
                        let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), obj.upcast()];
                        callback.call(&mut ctx, this, args)?;
                        wg_clone.done();
                        Ok(())
                    });
                }

            wg.wait();
            println!("{:?} with count {}", options, i);
            Ok(())
        }).promise(|mut tctx: TaskContext, _result: Result<(), DBError>| {
            // for _ in 0..100 {
            println!("?????????????????????????????????????????????????????????");
            // }
            Ok(tctx.undefined())
        });

        Ok(promise)
    }
}

pub type SharedDatabase = JsBox<DB>;

pub type Database = DB;

impl Database {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<DB>> {
        let mut option = rocksdb::Options::default();
        option.create_if_missing(true);
        let options = ctx.argument_opt(1);

        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let opts = DbOptions::new_with_context(&mut ctx, options)?;

        let db = if opts.is_readonly() {
            rocksdb::DB::open_for_read_only(&option, path, false)
        } else {
            rocksdb::DB::open(&option, path)
        }.unwrap();

        let db = DB::new(db, Kind::Normal);

        Ok(ctx.boxed(db))
    }
    /// js_clear is handler for JS ffi.
    /// js "this" - DB.
    /// - @params(0) - Options for range. {limit: u32, reverse: bool, gte: &[u8], lte: &[u8]}.
    /// - @params(1) - callback to return the result.
    /// - @callback(0) - Error.
    pub fn js_clear(mut ctx: FunctionContext) -> JsResult<JsPromise> {
        let db = ctx
            .this()
            .downcast_or_throw::<SharedDatabase, _>(&mut ctx)?;

        let cloned = db.arc_clone();

        let promise = ctx.task(move || {
            let locked = cloned.lock().expect("msg");
            let mut batch = rocksdb::WriteBatch::default();
            let conn_iter = locked.iterator(rocksdb::IteratorMode::Start);
            for key_val in conn_iter {
                batch.delete(&(key_val.unwrap().0));
            }
            locked.write(batch)?;
            Ok(())
        }).promise(|mut tctx: TaskContext, _result: Result<(), rocksdb::Error>| {
            Ok(tctx.undefined())
        });

        Ok(promise)
    }

    /// js_get is handler for JS ffi.
    /// js "this" - DB.
    /// - @params(0) - key to get from db.
    /// - @params(1) - callback to return the fetched value.
    /// - @callback(0) - Error. If data is not found, it will call the callback with "No data" as a first args.
    /// - @callback(1) - [u8]. Value associated with the key.
    pub fn js_get(mut ctx: FunctionContext) -> JsResult<JsPromise> {
        let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
        let db = ctx
            .this()
            .downcast_or_throw::<SharedDatabase, _>(&mut ctx)?;

        let cdb = db.arc_clone();
        let key = db.key(key);

        let promise = ctx.task(move || {
            let locked = cdb.try_lock().map_err(|_| DBError::LockError)?;
            let result = locked.get(&key).map_err(|e| DBError::RocksDBError(e.to_string()))?;
            result.ok_or(DBError::NotFound)
        }).promise(|mut tctx: TaskContext, result: Result<Vec<u8>, DBError>| {
            let value = result.or_else(|e| tctx.throw_error(e.to_string()))?;
            Ok(JsBuffer::external(&mut tctx, value))
        });

        Ok(promise)
    }

    /// js_exists is handler for JS ffi.
    /// js "this" - DB.
    /// - @params(0) - key to check existence from db.
    /// - @params(1) - callback to return the fetched value.
    /// - @callback(0) - Error
    /// - @callback(1) - bool
    // pub fn js_exists(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
    //     let key = ctx.argument::<JsTypedArray<u8>>(0)?.as_slice(&ctx).to_vec();
    //     let callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
    //     let db = ctx
    //         .this()
    //         .downcast_or_throw::<SharedDatabase, _>(&mut ctx)?;
    //     let db = db.borrow();

    //     db.exists(key, callback)
    //         .or_else(|err| ctx.throw_error(err.to_string()))?;

    //     Ok(ctx.undefined())
    // }
    pub fn js_iterate(mut ctx: FunctionContext) -> JsResult<JsPromise> {
        let option_inputs = ctx.argument::<JsObject>(0)?;
        let root_callback = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let options = IterationOption::new(&mut ctx, option_inputs);

        let db = ctx
            .this()
            .downcast_or_throw::<SharedDatabase, _>(&mut ctx)?;

        let cdb = db.arc_clone();

        let s_cb = StreamCallback{
            callback: CallbackWrapper(Arc::new(root_callback)),
            channel: Arc::new(ctx.channel()),
        };
        s_cb.invoke(&mut ctx, cdb, options)
    }

}
