use neon::prelude::*;

mod batch;
mod db;
mod state_db;
mod options;
mod smt;
mod smt_db;
mod in_memory_db;

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("db_new", db::Database::js_new)?;
    cx.export_function("db_clear", db::Database::js_clear)?;
    cx.export_function("db_close", db::Database::js_close)?;
    cx.export_function("db_get", db::Database::js_get)?;
    cx.export_function("db_set", db::Database::js_set)?;
    cx.export_function("db_del", db::Database::js_del)?;
    cx.export_function("db_write", db::Database::js_write)?;
    cx.export_function("db_iterate", db::Database::js_iterate)?;

    cx.export_function("batch_new", batch::WriteBatch::js_new)?;
    cx.export_function("batch_set", batch::WriteBatch::js_set)?;
    cx.export_function("batch_del", batch::WriteBatch::js_del)?;

    cx.export_function("state_db_new", state_db::StateDB::js_new)?;
    cx.export_function("state_db_close", state_db::StateDB::js_close)?;
    cx.export_function("state_db_get", state_db::StateDB::js_get)?;
    cx.export_function("state_db_set", state_db::StateDB::js_set)?;
    cx.export_function("state_db_del", state_db::StateDB::js_del)?;
    cx.export_function("state_db_clear", state_db::StateDB::js_clear)?;
    cx.export_function("state_db_snapshot", state_db::StateDB::js_snapshot)?;
    cx.export_function("state_db_snapshot_restore", state_db::StateDB::js_snapshot_restore)?;
    cx.export_function("state_db_iterate", state_db::StateDB::js_iterate)?;
    cx.export_function("state_db_revert", state_db::StateDB::js_revert)?;
    cx.export_function("state_db_commit", state_db::StateDB::js_commit)?;

    cx.export_function("in_memory_db_new", in_memory_db::Database::js_new)?;
    cx.export_function("in_memory_db_get", in_memory_db::Database::js_get)?;
    cx.export_function("in_memory_db_set", in_memory_db::Database::js_set)?;
    cx.export_function("in_memory_db_del", in_memory_db::Database::js_del)?;
    cx.export_function("in_memory_db_clear", in_memory_db::Database::js_clear)?;
    cx.export_function("in_memory_db_write", in_memory_db::Database::js_write)?;
    cx.export_function("in_memory_db_iterate", in_memory_db::Database::js_iterate)?;

    Ok(())
}
