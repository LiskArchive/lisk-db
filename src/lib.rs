use neon::prelude::*;

use crate::common_db::{JsNewWithArcMutex, JsNewWithBox, JsNewWithBoxRef};
pub use crate::types::{Cache, DbOptions, KeyLength, NestedVec, SharedKVPair};

pub mod batch;
mod codec;
mod common_db;
pub mod consts;
mod db;
mod diff;
mod in_memory_db;
mod in_memory_smt;
mod options;
pub mod smt;
pub mod smt_db;
mod state_db;
mod state_writer;
mod types;
mod utils;

extern crate tempdir;

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    cx.export_function(
        "db_new",
        db::Database::js_new_with_box::<DbOptions, db::Database>,
    )?;
    cx.export_function("db_clear", db::Database::js_clear)?;
    cx.export_function("db_close", db::Database::js_close)?;
    cx.export_function("db_get", db::Database::js_get)?;
    cx.export_function("db_exists", db::Database::js_exists)?;
    cx.export_function("db_set", db::Database::js_set)?;
    cx.export_function("db_del", db::Database::js_del)?;
    cx.export_function("db_write", db::Database::js_write)?;
    cx.export_function("db_iterate", db::Database::js_iterate)?;
    cx.export_function("db_checkpoint", db::Database::js_checkpoint)?;

    cx.export_function(
        "batch_new",
        batch::WriteBatch::js_new_with_arc_mutx::<batch::WriteBatch>,
    )?;
    cx.export_function("batch_set", batch::WriteBatch::js_set)?;
    cx.export_function("batch_del", batch::WriteBatch::js_del)?;

    cx.export_function(
        "state_db_new",
        state_db::StateDB::js_new_with_box_ref::<DbOptions, state_db::StateDB>,
    )?;
    cx.export_function(
        "state_db_get_current_state",
        state_db::StateDB::js_get_current_state,
    )?;
    cx.export_function("state_db_close", state_db::StateDB::js_close)?;
    cx.export_function("state_db_get", state_db::StateDB::js_get)?;
    cx.export_function("state_db_exists", state_db::StateDB::js_exists)?;
    cx.export_function("state_db_iterate", state_db::StateDB::js_iterate)?;
    cx.export_function("state_db_revert", state_db::StateDB::js_revert)?;
    cx.export_function("state_db_commit", state_db::StateDB::js_commit)?;
    cx.export_function("state_db_prove", state_db::StateDB::js_prove)?;
    cx.export_function("state_db_verify", state_db::StateDB::js_verify)?;
    cx.export_function("state_db_range_with_writer", state_db::StateDB::js_range)?;
    cx.export_function(
        "state_db_js_upsert_key_with_writer",
        state_db::StateDB::js_upsert_key,
    )?;
    cx.export_function(
        "state_db_js_delete_key_with_writer",
        state_db::StateDB::js_delete_key,
    )?;
    cx.export_function(
        "state_db_js_get_key_with_writer",
        state_db::StateDB::js_get_key,
    )?;
    cx.export_function(
        "state_db_clean_diff_until",
        state_db::StateDB::js_clean_diff_until,
    )?;
    cx.export_function("state_db_checkpoint", state_db::StateDB::js_checkpoint)?;

    cx.export_function(
        "state_writer_new",
        state_writer::StateWriter::js_new_with_arc_mutx::<state_writer::StateWriter>,
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

    cx.export_function(
        "in_memory_smt_new",
        in_memory_smt::InMemorySMT::js_new_with_arc_mutx::<in_memory_smt::InMemorySMT>,
    )?;
    cx.export_function(
        "in_memory_smt_update",
        in_memory_smt::InMemorySMT::js_update,
    )?;
    cx.export_function("in_memory_smt_prove", in_memory_smt::InMemorySMT::js_prove)?;
    cx.export_function(
        "in_memory_smt_verify",
        in_memory_smt::InMemorySMT::js_verify,
    )?;

    Ok(())
}
