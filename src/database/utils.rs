/// utils provides common functionalities used in the DB, such as parsing JS context, iteration checks.
use std::cmp;

use neon::context::Context;
use neon::handle::Handle;
use neon::object::Object;
use neon::result::NeonResult;
use neon::types::{JsArray, JsBuffer, JsObject, JsValue};

use crate::consts::Prefix;
use crate::database::options;
use crate::state_writer;
use crate::types::{Cache, KVPair};
use crate::utils::compare;

pub fn pair_to_js_object<'a, C: Context<'a>>(
    ctx: &mut C,
    pair: &KVPair,
) -> NeonResult<Handle<'a, JsObject>> {
    let obj = ctx.empty_object();
    let key = JsBuffer::external(ctx, pair.key_as_vec());
    obj.set(ctx, "key", key)?;
    let value = JsBuffer::external(ctx, pair.value_as_vec());
    obj.set(ctx, "value", value)?;

    Ok(obj)
}

pub fn parse_update_result<'a, C: Context<'a>>(
    ctx: &mut C,
    result: Result<(), state_writer::StateWriterError>,
) -> NeonResult<Vec<Handle<'a, JsValue>>> {
    if result.is_err() {
        let err = result.err().unwrap().to_string();
        Ok(vec![ctx.error(err)?.upcast()])
    } else {
        Ok(vec![ctx.null().upcast()])
    }
}

pub fn cache_to_js_array<'a, C: Context<'a>>(
    ctx: &mut C,
    values: &Cache,
) -> NeonResult<Handle<'a, JsArray>> {
    let res_values = ctx.empty_array();
    for (i, kv) in values.iter().enumerate() {
        let temp_pair = KVPair::new(kv.0, kv.1);
        let object = pair_to_js_object(ctx, &temp_pair)?;
        res_values.set(ctx, i as u32, object)?;
    }

    Ok(res_values)
}

pub fn get_iteration_mode<'a>(
    options: &options::IterationOption,
    opt: &'a mut Vec<u8>,
    has_prefix: bool,
) -> rocksdb::IteratorMode<'a> {
    let no_range = options.gte.is_none() && options.lte.is_none();
    if no_range {
        if options.reverse {
            rocksdb::IteratorMode::End
        } else {
            rocksdb::IteratorMode::Start
        }
    } else if options.reverse {
        let lte = options
            .lte
            .clone()
            .unwrap_or_else(|| vec![255; options.gte.as_ref().unwrap().len()]);
        *opt = if has_prefix {
            [Prefix::STATE, lte.as_slice()].concat()
        } else {
            lte
        };
        rocksdb::IteratorMode::From(opt, rocksdb::Direction::Reverse)
    } else {
        let gte = options
            .gte
            .clone()
            .unwrap_or_else(|| vec![0; options.lte.as_ref().unwrap().len()]);
        *opt = if has_prefix {
            [Prefix::STATE, gte.as_slice()].concat()
        } else {
            gte
        };
        rocksdb::IteratorMode::From(opt, rocksdb::Direction::Forward)
    }
}

pub fn is_key_out_of_range(
    options: &options::IterationOption,
    key: &[u8],
    counter: i64,
    has_prefix: bool,
) -> bool {
    if options.limit != -1 && counter >= options.limit {
        return true;
    }
    if options.reverse {
        if let Some(gte) = &options.gte {
            let cmp = if has_prefix {
                [Prefix::STATE, gte].concat()
            } else {
                gte.to_vec()
            };
            if compare(key, &cmp) == cmp::Ordering::Less {
                return true;
            }
        }
    } else if let Some(lte) = &options.lte {
        let cmp = if has_prefix {
            [Prefix::STATE, lte].concat()
        } else {
            lte.to_vec()
        };
        if compare(key, &cmp) == cmp::Ordering::Greater {
            return true;
        }
    }

    false
}
