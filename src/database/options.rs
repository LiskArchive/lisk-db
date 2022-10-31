/// options provides functionality to read Database open and iteration options.
use neon::prelude::*;
use neon::types::buffer::TypedArray;

use crate::consts;
use crate::database::traits::OptionsWithContext;
use crate::database::types::DbOptions;
use crate::types::{KeyLength, VecOption};

/// IterationOption holds iterator option for the database.
#[derive(Clone, Debug)]
pub struct IterationOption {
    pub limit: i64,
    pub reverse: bool,
    pub gte: VecOption,
    pub lte: VecOption,
}

impl OptionsWithContext for DbOptions {
    fn new_with_context<'a, C>(
        ctx: &mut C,
        input: Option<Handle<JsValue>>,
    ) -> Result<Self, neon::result::Throw>
    where
        C: Context<'a>,
    {
        if input.is_none() {
            return Ok(Self::default());
        }
        let obj = input.unwrap().downcast_or_throw::<JsObject, _>(ctx)?;
        let readonly = obj
            .get_opt::<JsBoolean, _, _>(ctx, "readonly")?
            .map(|val| {
                val.downcast::<JsBoolean, _>(ctx)
                    .map(|val| val.value(ctx))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let key_length = KeyLength(
            obj.get_opt::<JsNumber, _, _>(ctx, "keyLength")?
                .map(|val| {
                    val.downcast::<JsNumber, _>(ctx)
                        .map(|val| val.value(ctx) as u16)
                        .unwrap_or_else(|_| consts::KEY_LENGTH.into())
                })
                .unwrap_or_else(|| consts::KEY_LENGTH.into()),
        );

        Ok(Self::new(readonly, key_length))
    }
}

impl Default for DbOptions {
    fn default() -> Self {
        Self::new(false, consts::KEY_LENGTH)
    }
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
            .map(|val| val.map(|v| v.as_slice(ctx).to_vec()))
            .unwrap_or(None);

        let lte = input
            .get_opt::<JsTypedArray<u8>, _, _>(ctx, "lte")
            .map(|val| val.map(|v| v.as_slice(ctx).to_vec()))
            .unwrap_or(None);

        Self {
            limit: limit as i64,
            reverse,
            gte,
            lte,
        }
    }
}
