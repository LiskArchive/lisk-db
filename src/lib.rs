use std::sync::mpsc;
use std::thread;

use neon::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    message: String,
}

struct Database {
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
    fn js_new(mut ctx: FunctionContext) -> JsResult<JsBox<Database>> {
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

    fn js_close(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
        // Get the `this` value as a `JsBox<Database>`
        ctx.this()
            .downcast_or_throw::<JsBox<Database>, _>(&mut ctx)?
            .close()
            .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }

    fn js_get(mut ctx: FunctionContext) -> JsResult<JsUndefined> {
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
                    Ok(val) => match val {
                        Some(v) => {
                            let buffer = JsBuffer::external(&mut ctx, v);
                            vec![ctx.null().upcast(), buffer.upcast()]
                        }
                        None => vec![ctx.error("No data")?.upcast()],
                    },
                    Err(err) => vec![ctx.error(err.to_string())?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
        .or_else(|err| ctx.throw_error(err.to_string()))?;

        Ok(ctx.undefined())
    }
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("db_new", Database::js_new)?;
    cx.export_function("db_close", Database::js_close)?;
    cx.export_function("db_get", Database::js_get)?;
    Ok(())
}
