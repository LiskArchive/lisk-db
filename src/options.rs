use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::consts;

#[derive(Debug)]
pub struct DatabaseOptions {
    pub readonly: bool,
    pub key_length: usize,
}

impl DatabaseOptions {
    pub fn new<'a, C>(
        ctx: &mut C,
        input: Option<Handle<JsValue>>,
    ) -> Result<Self, neon::result::Throw>
    where
        C: Context<'a>,
    {
        if input.is_none() {
            return Ok(Self {
                readonly: false,
                key_length: consts::KEY_LENGTH,
            });
        }
        let obj = input.unwrap().downcast_or_throw::<JsObject, _>(ctx)?;
        let readonly = obj
            .get_opt::<JsBoolean, _, _>(ctx, "readonly")?
            .map(|val| {
                val.downcast::<JsBoolean, _>(ctx)
                    .and_then(|val| Ok(val.value(ctx)))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let key_length = obj
            .get_opt::<JsNumber, _, _>(ctx, "keyLength")?
            .map(|val| {
                val.downcast::<JsNumber, _>(ctx)
                    .and_then(|val| Ok(val.value(ctx) as usize))
                    .unwrap_or(consts::KEY_LENGTH)
            })
            .unwrap_or(consts::KEY_LENGTH);

        Ok(Self {
            readonly: readonly,
            key_length: key_length,
        })
    }
}

pub type DbCallback = Box<dyn FnOnce(&mut rocksdb::DB, &Channel) + Send>;

// Messages sent on the database channel
pub enum DbMessage {
    // Callback to be executed
    Callback(DbCallback),
    // Indicates that the thread should be stopped and connection closed
    Close,
}

#[derive(Clone, Debug)]
pub struct IterationOption {
    pub limit: i64,
    pub reverse: bool,
    pub gte: Option<Vec<u8>>,
    pub lte: Option<Vec<u8>>,
}

impl IterationOption {
    pub fn new<'a, C>(ctx: &mut C, input: Handle<JsObject>) -> Self
    where
        C: Context<'a>,
    {
        let reverse = input
            .get_opt::<JsBoolean, _, _>(ctx, "reverse")
            .map(|val| match val {
                Some(v) => v.value(ctx),
                None => false,
            })
            .unwrap_or(false);
        let limit = input
            .get_opt::<JsNumber, _, _>(ctx, "limit")
            .map(|val| match val {
                Some(v) => v.value(ctx),
                None => -1.0,
            })
            .unwrap_or(-1.0);

        let gte = input
            .get_opt::<JsTypedArray<u8>, _, _>(ctx, "gte")
            .map(|val| match val {
                Some(v) => Some(v.as_slice(ctx).to_vec()),
                None => None,
            })
            .unwrap_or(None);

        let lte = input
            .get_opt::<JsTypedArray<u8>, _, _>(ctx, "lte")
            .map(|val| match val {
                Some(v) => Some(v.as_slice(ctx).to_vec()),
                None => None,
            })
            .unwrap_or(None);

        Self {
            limit: limit as i64,
            reverse: reverse,
            gte: gte,
            lte: lte,
        }
    }
}
