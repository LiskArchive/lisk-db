use std::cell::RefCell;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use neon::context::{Context, FunctionContext};
use neon::event::Channel;
use neon::handle::{Handle, Root};
use neon::result::JsResult;
use neon::types::{Finalize, JsBox, JsBuffer, JsFunction, JsNumber, JsString, JsValue};
use rocksdb::checkpoint::Checkpoint;

use crate::consts;
use crate::options::DbMessage;
use crate::types::{ArcMutex, KVPair, KeyLength, New, VecOption};

pub type JsBoxRef<T> = JsBox<RefCell<T>>;
pub type JsArcMutex<T> = JsBoxRef<ArcMutex<T>>;

// Kind represented the kind of the database
#[derive(PartialEq, Eq)]
pub enum Kind {
    Normal,
    State,
    StateWriter,
    Batch,
    InMemorySMT,
    InMemoryDB,
}

#[derive(Debug, Copy, Clone)]
pub struct Options {
    pub readonly: bool,
    pub key_length: KeyLength,
}

pub struct DB {
    tx: mpsc::Sender<DbMessage>,
    db_kind: Kind,
}

pub trait Actions {
    fn get(&self, key: &[u8]) -> Result<VecOption, rocksdb::Error>;
    fn set(&mut self, pair: &KVPair) -> Result<(), rocksdb::Error>;
    fn del(&mut self, key: &[u8]) -> Result<(), rocksdb::Error>;
}

pub trait NewDBWithKeyLength {
    fn new_db_with_key_length(len: Option<KeyLength>) -> Self;
}

pub trait DatabaseKind {
    fn db_kind() -> Kind;
}

pub trait OptionsWithContext {
    fn new_with_context<'a, C>(
        ctx: &mut C,
        input: Option<Handle<JsValue>>,
    ) -> Result<Options, neon::result::Throw>
    where
        C: Context<'a>,
        Self: Sized;
}

pub trait NewDBWithContext {
    fn new_db_with_context<'a, C>(
        ctx: &mut C,
        path: String,
        opts: Options,
        db_kind: Kind,
    ) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
        Self: Sized;
}

pub trait JsNewWithBoxRef {
    fn js_new_with_box_ref<T: OptionsWithContext, U: NewDBWithContext + Send + Finalize>(
        mut ctx: FunctionContext,
    ) -> JsResult<JsBoxRef<U>> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let options = ctx.argument_opt(1);
        let db_opts = T::new_with_context(&mut ctx, options)?;
        let db = U::new_db_with_context(&mut ctx, path, db_opts, Kind::State)
            .or_else(|err| ctx.throw_error(&err))?;
        let ref_db = RefCell::new(db);

        return Ok(ctx.boxed(ref_db));
    }
}

pub trait JsNewWithBox {
    fn js_new_with_box<T: OptionsWithContext, U: NewDBWithContext + Finalize + Send>(
        mut ctx: FunctionContext,
    ) -> JsResult<JsBox<U>> {
        let path = ctx.argument::<JsString>(0)?.value(&mut ctx);
        let options = ctx.argument_opt(1);
        let db_opts = T::new_with_context(&mut ctx, options)?;
        let db = U::new_db_with_context(&mut ctx, path, db_opts, Kind::Normal)
            .or_else(|err| ctx.throw_error(&err))?;

        return Ok(ctx.boxed(db));
    }
}

pub trait JsNewWithArcMutex {
    fn js_new_with_arc_mutx<T: NewDBWithKeyLength + Send + Finalize + DatabaseKind>(
        mut ctx: FunctionContext,
    ) -> JsResult<JsArcMutex<T>> {
        let key_length = if T::db_kind() == Kind::InMemorySMT {
            Some(ctx.argument::<JsNumber>(0)?.value(&mut ctx).into())
        } else {
            None
        };
        let ref_tree = RefCell::new(Arc::new(Mutex::new(T::new_db_with_key_length(key_length))));
        return Ok(ctx.boxed(ref_tree));
    }
}

impl New for Options {
    fn new() -> Self {
        Self {
            readonly: false,
            key_length: KeyLength(0),
        }
    }
}

impl NewDBWithContext for DB {
    fn new_db_with_context<'a, C>(
        ctx: &mut C,
        path: String,
        opts: Options,
        db_kind: Kind,
    ) -> Result<Self, rocksdb::Error>
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
                    },
                    DbMessage::Close => return,
                }
            }
        });

        Ok(Self { tx, db_kind })
    }
}

impl Kind {
    pub fn key(&self, key: Vec<u8>) -> Vec<u8> {
        match self {
            Kind::State => [consts::PREFIX_STATE, &key].concat(),
            _ => key,
        }
    }
}

impl Finalize for DB {}
impl DB {
    // Idiomatic rust would take an owned `self` to prevent use after close
    // However, it's not possible to prevent JavaScript from continuing to hold a closed database
    pub fn close(&self) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx.send(DbMessage::Close)
    }

    pub fn send(
        &self,
        callback: impl FnOnce(&mut rocksdb::DB, &Channel) + Send + 'static,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx.send(DbMessage::Callback(Box::new(callback)))
    }

    pub fn get_by_key(
        &self,
        key: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let key = self.db_kind.key(key);
        self.send(move |conn, channel| {
            let result = conn.get(&key);

            channel.send(move |mut ctx| {
                let callback = cb.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(Some(val)) => {
                        let buffer = JsBuffer::external(&mut ctx, val);
                        vec![ctx.null().upcast(), buffer.upcast()]
                    },
                    Ok(None) => vec![ctx.error("No data")?.upcast()],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    pub fn exists(
        &self,
        key: Vec<u8>,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let key = self.db_kind.key(key);
        self.send(move |conn, channel| {
            let result = if conn.key_may_exist(&key) {
                conn.get(&key).map(|res| res.is_some())
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
                    },
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    pub fn checkpoint(
        &self,
        path: String,
        cb: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        self.send(move |conn, channel| {
            let result = Checkpoint::new(conn);

            if result.is_err() {
                let err = result.err().unwrap();
                channel.send(move |mut ctx| {
                    let callback = cb.into_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args = vec![ctx.error(&err)?.upcast()];

                    callback.call(&mut ctx, this, args)?;

                    Ok(())
                });
            } else if let Ok(checkpoint) = result {
                let result = checkpoint.create_checkpoint(&path);

                channel.send(move |mut ctx| {
                    let callback = cb.into_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args: Vec<Handle<JsValue>> = match result {
                        Ok(()) => {
                            vec![ctx.null().upcast()]
                        },
                        Err(err) => vec![ctx.error(&err)?.upcast()],
                    };

                    callback.call(&mut ctx, this, args)?;

                    Ok(())
                });
            }
        })
    }
}
