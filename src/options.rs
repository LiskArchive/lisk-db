use neon::prelude::*;
use std::cmp;

#[derive(Debug)]
pub struct DatabaseOptions {
    pub readonly: bool,
}

impl DatabaseOptions {
    pub fn new() -> DatabaseOptions {
        Self { readonly: false }
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
    pub start: Option<Vec<u8>>,
    pub end: Option<Vec<u8>>,
}

impl IterationOption {
    pub fn new<'a, C>(ctx: &mut C, input: Handle<JsObject>) -> Self
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

pub fn compare(a: &[u8], b: &[u8]) -> cmp::Ordering {
    for (ai, bi) in a.iter().zip(b.iter()) {
        match ai.cmp(&bi) {
            cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    /* if every single element was equal, compare length */
    a.len().cmp(&b.len())
}