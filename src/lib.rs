use neon::prelude::*;

mod batch;
mod codec;
mod consts;
mod db;
mod diff;
mod in_memory_db;
mod options;
mod smt;
mod smt_db;
mod state_db;
mod state_writer;
mod utils;

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function("db_new", db::Database::js_new)?;
    cx.export_function("db_clear", db::Database::js_clear)?;
    cx.export_function("db_close", db::Database::js_close)?;
    cx.export_function("db_get", db::Database::js_get)?;
    cx.export_function("db_exists", db::Database::js_exists)?;
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
    cx.export_function("state_db_exists", state_db::StateDB::js_exists)?;
    cx.export_function("state_db_iterate", state_db::StateDB::js_iterate)?;
    cx.export_function("state_db_revert", state_db::StateDB::js_revert)?;
    cx.export_function("state_db_commit", state_db::StateDB::js_commit)?;
    cx.export_function("state_db_prove", state_db::StateDB::js_prove)?;
    cx.export_function("state_db_verify", state_db::StateDB::js_verify)?;
    cx.export_function(
        "state_db_clean_diff_until",
        state_db::StateDB::js_clean_diff_until,
    )?;

    cx.export_function("state_writer_new", state_writer::StateWriter::js_new)?;
    cx.export_function("state_writer_get", state_writer::StateWriter::js_get)?;
    cx.export_function("state_writer_update", state_writer::StateWriter::js_update)?;
    cx.export_function("state_writer_del", state_writer::StateWriter::js_del)?;
    cx.export_function(
        "state_writer_is_cached",
        state_writer::StateWriter::js_is_cached,
    )?;
    cx.export_function(
        "state_writer_get_range",
        state_writer::StateWriter::js_get_range,
    )?;
    cx.export_function(
        "state_writer_cache_new",
        state_writer::StateWriter::js_cache_new,
    )?;
    cx.export_function(
        "state_writer_cache_existing",
        state_writer::StateWriter::js_cache_existing,
    )?;
    cx.export_function(
        "state_writer_snapshot",
        state_writer::StateWriter::js_snapshot,
    )?;
    cx.export_function(
        "state_writer_restore_snapshot",
        state_writer::StateWriter::js_restore_snapshot,
    )?;

    cx.export_function("in_memory_db_new", in_memory_db::Database::js_new)?;
    cx.export_function("in_memory_db_clone", in_memory_db::Database::js_clone)?;
    cx.export_function("in_memory_db_get", in_memory_db::Database::js_get)?;
    cx.export_function("in_memory_db_set", in_memory_db::Database::js_set)?;
    cx.export_function("in_memory_db_del", in_memory_db::Database::js_del)?;
    cx.export_function("in_memory_db_clear", in_memory_db::Database::js_clear)?;
    cx.export_function("in_memory_db_write", in_memory_db::Database::js_write)?;
    cx.export_function("in_memory_db_iterate", in_memory_db::Database::js_iterate)?;

    cx.export_function("in_memory_smt_new", smt::InMemorySMT::js_new)?;
    cx.export_function("in_memory_smt_update", smt::InMemorySMT::js_update)?;
    cx.export_function("in_memory_smt_prove", smt::InMemorySMT::js_prove)?;
    cx.export_function("in_memory_smt_verify", smt::InMemorySMT::js_verify)?;

    Ok(())
}
