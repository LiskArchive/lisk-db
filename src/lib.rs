use neon::prelude::*;

use crate::database::db;

pub mod batch;
pub mod consts;
pub mod database;
pub mod types;

use db::Database;

/// main registers functions for JS ffi

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("db_new", Database::js_new)?;
    cx.export_function("db_clear", Database::js_clear)?;
    cx.export_function("db_get", Database::js_get)?;
    cx.export_function("db_iterate", Database::js_iterate)?;

    Ok(())
}
