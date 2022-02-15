use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::cmp;

use neon::prelude::*;

use crate::batch;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    message: String,
}

pub struct Database {
    tx: mpsc::Sender<DbMessage>,
}

impl Finalize for Database {}

#[derive(Debug)]
struct DatabaseOptions {
    readonly: bool,
}

impl DatabaseOptions {
    fn new() -> DatabaseOptions {
        Self { readonly: false }
    }
}

type DbCallback = Box<dyn FnOnce(&mut rocksdb::DB, &Channel) + Send>;

// Messages sent on the database channel
enum DbMessage {
    // Callback to be executed
    Callback(DbCallback),
    // Indicates that the thread should be stopped and connection closed
    Close,
}

fn compare(a: &[u8], b: &[u8]) -> cmp::Ordering {
    for (ai, bi) in a.iter().zip(b.iter()) {
        match ai.cmp(&bi) {
            cmp::Ordering::Equal => continue,
            ord => return ord
        }
    }
    /* if every single element was equal, compare length */
    a.len().cmp(&b.len())
}

impl Database {
    fn new<'a, C>(ctx: &mut C, path: String, opts: DatabaseOptions) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
    {
        // Channel for sending callbacks to execute on the sqlite connection thread
        let (tx, rx) = mpsc::channel::<DbMessage>();

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
                    DbMessage::Callback(f) => {
                        f(&mut opened, &channel);
                    }
                    DbMessage::Close => break,
                }
            }
        });

        return Ok(Self { tx: tx });
    }

    // Idiomatic rust would take an owned `self` to prevent use after close
    // However, it's not possible to prevent JavaScript from continuing to hold a closed database
    fn close(&self) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx.send(DbMessage::Close)
    }

    fn send(
        &self,
        callback: impl FnOnce(&mut rocksdb::DB, &Channel) + Send + 'static,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx.send(DbMessage::Callback(Box::new(callback)))
    }
}

impl Database {
    pub fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<Database>> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let options = ctx.argument_opt(1);
        let mut db_opts = DatabaseOptions::new();
        if let Some(options) = options {
            let obj = options.downcast_or_throw::<JsObject, _>(&mut ctx)?;
            let readonly = obj
                .get(&mut ctx, "readonly")?
                .downcast::<JsBoolean, _>(&mut ctx);
            db_opts.readonly = match readonly {
                Ok(readonly) => readonly.value(&mut ctx),
                Err(_) => false,
            };
            println!("{:?}", db_opts);
        }
        let db = Database::new(&mut ctx, path, db_opts)
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        return Ok(ctx.boxed(db));
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
        let mut buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut buf, |data| data.as_slice().to_vec());
        let cb = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`
        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        db.send(move |conn, channel| {
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
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    pub fn js_set(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        let mut key_buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut key_buf, |data| data.as_slice().to_vec());
        let mut value_buf = ctx.argument::<JsBuffer>(1)?;
        let value = ctx.borrow(&mut value_buf, |data| data.as_slice().to_vec());
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
        let mut key_buf = ctx.argument::<JsBuffer>(0)?;
        let key = ctx.borrow(&mut key_buf, |data| data.as_slice().to_vec());
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
        let options = IterationOption::new(&mut ctx, option_inputs);
        let cb_on_data = ctx.argument::<JsFunction>(1)?.root(&mut ctx);
        let cb_done = ctx.argument::<JsFunction>(2)?.root(&mut ctx);
        // Get the `this` value as a `JsBox<Database>`

        let db = ctx
            .this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?;

        let a_cb_on_data = Arc::new(Mutex::new(cb_on_data));
        db.send(move |conn, channel| {
            let no_range = options.start.is_none() && options.end.is_none();
            let iter;
            if no_range {
                if options.reverse {
                    iter = conn.iterator(rocksdb::IteratorMode::End);
                } else {
                    iter = conn.iterator(rocksdb::IteratorMode::Start);
                }
            } else {
                let start = match options.start.clone() {
                    Some(v) => v,
                    None => {
                        if options.reverse {
                            vec![255; options.end.clone().unwrap().len()]
                        } else {
                            vec![0; options.end.clone().unwrap().len()]
                        }
                    },
                };
                if options.reverse {
                    iter = conn.iterator(rocksdb::IteratorMode::From(&start, rocksdb::Direction::Reverse));
                } else {
                    iter = conn.iterator(rocksdb::IteratorMode::From(&start, rocksdb::Direction::Forward));
                }
            }
            let mut counter = 0;
            for (key, val) in iter {
                if options.limit != -1 && counter >= options.limit {
                    break;
                }
                if let Some(end) = options.end.clone() {
                    if options.reverse && compare(&key, &end) == cmp::Ordering::Less {
                        break;
                    }
                    if !options.reverse && compare(&key, &end) == cmp::Ordering::Greater {
                        break;
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

struct IterationOption {
    limit: i64,
    reverse: bool,
    start: Option<Vec<u8>>,
    end: Option<Vec<u8>>,
}

impl IterationOption {
    fn new<'a, C>(ctx: &mut C, input: Handle<JsObject>) -> Self
    where
        C: Context<'a>,
    {
        let reverse = input
            .get(ctx, "reverse")
            .map(|val| {
                val.downcast::<JsBoolean, _>(ctx)
                    .and_then(|val| Ok(val.value(ctx)))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let limit = input
            .get(ctx, "limit")
            .map(|val| {
                val.downcast::<JsNumber, _>(ctx)
                    .and_then(|val| Ok(val.value(ctx)))
                    .unwrap_or(-1.0)
            })
            .unwrap_or(-1.0);

        let start = input
            .get(ctx, "start")
            .map(|val| {
                val.downcast::<JsBuffer, _>(ctx)
                    .map(|mut val| {
                        ctx.borrow(&mut val, |data| Some(data.as_slice::<u8>().to_vec()))
                    })
                    .unwrap_or(None)
            })
            .unwrap_or(None);

        let end = input
            .get(ctx, "end")
            .map(|val| {
                val.downcast::<JsBuffer, _>(ctx)
                    .map(|mut val| {
                        ctx.borrow(&mut val, |data| Some(data.as_slice::<u8>().to_vec()))
                    })
                    .unwrap_or(None)
            })
            .unwrap_or(None);

        Self {
            limit: limit as i64,
            reverse: reverse,
            start: start,
            end: end,
        }
    }
}