use std::cmp;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::batch;
use crate::options;
use crate::utils;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    message: String,
}

pub struct Database {
    tx: mpsc::Sender<options::DbMessage>,
}

impl Finalize for Database {}

impl Database {
    fn new<'a, C>(
        ctx: &mut C,
        path: String,
        opts: options::DatabaseOptions,
    ) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
    {
        // Channel for sending callbacks to execute on the sqlite connection thread
        let (tx, rx) = mpsc::channel::<options::DbMessage>();

        let channel = ctx.channel();

        let mut option = rocksdb::Options::default();
        option.create_if_missing(true);

        let mut opened: rocksdb::DB;
        if opts.readonly {
            opened = rocksdb::DB::open_for_read_only(&option, path, false)?;
        } else {
            opened = rocksdb::DB::open(&option, path)?;
        }

        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                match message {
                    options::DbMessage::Callback(f) => {
                        f(&mut opened, &channel);
                    }
                    options::DbMessage::Close => return,
                }
            }
        });

        return Ok(Self { tx: tx });
    }

    // Idiomatic rust would take an owned `self` to prevent use after close
    // However, it's not possible to prevent JavaScript from continuing to hold a closed database
    fn close(&self) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.tx.send(options::DbMessage::Close)
    }

    fn send(
        &self,
        callback: impl FnOnce(&mut rocksdb::DB, &Channel) + Send + 'static,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.tx
            .send(options::DbMessage::Callback(Box::new(callback)))
    }

    fn get_by_key(
        &self,
        key: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.send(move |conn, channel| {
            let result = conn.get(key);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(Some(val)) => {
                        let buffer = JsBuffer::external(&mut ctx, val);
                        vec![ctx.null().upcast(), buffer.upcast()]
                    }
                    Ok(None) => vec![ctx.error("No data")?.upcast()],
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    fn exists(
        &self,
        key: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<options::DbMessage>> {
        self.send(move |conn, channel| {
            let exist = conn.key_may_exist(&key);
            let result = if exist {
                conn.get(&key).and_then(|res| Ok(res.is_some()))
            } else {
                Ok(false)
            };

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let converted = ctx.boolean(val);
                        vec![ctx.null().upcast(), converted.upcast()]
                    }
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }
}

impl Database {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<Database>> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let options = ctx.argument_opt(1);
        let db_opts = options::DatabaseOptions::new(&mut ctx, options)?;
        let db = Database::new(&mut ctx, path, db_opts)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        return Ok(ctx.boxed(db));
    }

    pub fn js_clear(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        db.send(move |conn, channel| {
            let mut batch = rocksdb::WriteBatch::default();
            let iter = conn.iterator(rocksdb::IteratorMode::Start);
            for (key, _) in iter {
                batch.delete(&key);
            }
            let result = conn.write(batch);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
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

        db.send(move |conn, channel| {
            let result = conn.put(key, value);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
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

        db.send(move |conn, channel| {
            let result = conn.delete(key);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(_) => vec![ctx.null().upcast()],
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_write(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let batch = ctx.argument::<JsBox<batch::SendableWriteBatch>>(0)?;
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let batch = batch.borrow().clone();

        db.send(move |conn, channel| {
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
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
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
        let options = options::IterationOption::new(&mut ctx, option_inputs);
        let cb_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let cb_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let a_cb_on_data = Arc::new(Mutex::new(cb_on_data));
        db.send(move |conn, channel| {
            let no_range = options.gte.is_none() && options.lte.is_none();
            let iter;
            if no_range {
                if options.reverse {
                    iter = conn.iterator(rocksdb::IteratorMode::End);
                } else {
                    iter = conn.iterator(rocksdb::IteratorMode::Start);
                }
            } else {
                if options.reverse {
                    let lte = options
                        .lte
                        .clone()
                        .unwrap_or_else(|| vec![255; options.gte.clone().unwrap().len()]);
                    iter = conn.iterator(rocksdb::IteratorMode::From(
                        &lte,
                        rocksdb::Direction::Reverse,
                    ));
                } else {
                    let gte = options
                        .gte
                        .clone()
                        .unwrap_or_else(|| vec![0; options.lte.clone().unwrap().len()]);
                    iter = conn.iterator(rocksdb::IteratorMode::From(
                        &gte,
                        rocksdb::Direction::Forward,
                    ));
                }
            }
            let mut counter = 0;
            for (key, val) in iter {
                if options.limit != -1 && counter >= options.limit {
                    break;
                }
                if options.reverse {
                    if let Some(gte) = &options.gte {
                        if utils::compare(&key, &gte) == cmp::Ordering::Less {
                            break;
                        }
                    }
                } else {
                    if let Some(lte) = &options.lte {
                        if utils::compare(&key, &lte) == cmp::Ordering::Greater {
                            break;
                        }
                    }
                }
                let c = a_cb_on_data.clone();
                channel.send(move |mut ctx| {
                    let obj = ctx.empty_object();
                    let key_res = JsBuffer::external(&mut ctx, key);
                    let val_res = JsBuffer::external(&mut ctx, val);
                    obj.set(&mut ctx, "key", key_res)?;
                    obj.set(&mut ctx, "value", val_res)?;
                    let cb = c.lock().unwrap().to_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args: Vec<Handle<JsValue>> = vec![ctx.null().upcast(), obj.upcast()];
                    cb.call(&mut ctx, this, args)?;
                    Ok(())
                });
                counter += 1;
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
