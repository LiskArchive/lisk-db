use neon::prelude::*;

mod batch;
mod db;

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("db_new", db::Database::js_new)?;
    cx.export_function("db_close", db::Database::js_close)?;
    cx.export_function("db_get", db::Database::js_get)?;
    cx.export_function("db_set", db::Database::js_set)?;
    cx.export_function("db_del", db::Database::js_del)?;
    cx.export_function("db_write", db::Database::js_write)?;
    cx.export_function("db_iterate", db::Database::js_iterate)?;

    cx.export_function("batch_new", batch::WriteBatch::js_new)?;
    cx.export_function("batch_set", batch::WriteBatch::js_set)?;
    cx.export_function("batch_del", batch::WriteBatch::js_del)?;
    Ok(())
}
