/// traits provides common traits for database.
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use neon::context::{Context, FunctionContext};
use neon::handle::Handle;
use neon::result::JsResult;
use neon::types::{Finalize, JsNumber, JsString, JsValue};

use crate::database::types::{DbOptions, JsArcMutex, JsBoxRef, Kind};
use crate::types::{KVPair, KeyLength, VecOption};

pub trait Unwrap {
    fn unwrap(&self) -> &rocksdb::DB;
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
    ) -> Result<DbOptions, neon::result::Throw>
    where
        C: Context<'a>,
        Self: Sized;
}

pub trait NewDBWithContext {
    fn new_db_with_context<'a, C>(
        ctx: &mut C,
        path: String,
        opts: DbOptions,
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
        let db = U::new_db_with_context(&mut ctx, path, db_opts, Kind::Normal)
            .or_else(|err| ctx.throw_error(&err))?;
        let ref_db = RefCell::new(db);

        return Ok(ctx.boxed(ref_db));
    }
}

pub trait JsNewWithArcMutex {
    fn js_new_with_arc_mutex<T: NewDBWithKeyLength + Send + Finalize + DatabaseKind>(
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
