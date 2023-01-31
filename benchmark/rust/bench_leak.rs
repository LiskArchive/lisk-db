use std::convert::TryInto;
use std::error::Error;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time;
// use std::thread;

use rand::RngCore;
use rocksdb::{Options, DB};
use tempdir::TempDir;

use lisk_db::database::reader_writer::read_writer_db::ReadWriter;
use lisk_db::database::types::{DbMessage, Kind, SnapshotMessage};
use lisk_db::database::DB as LDB;
use lisk_db::state::state_db;
use lisk_db::state::state_db::{Commit, CommitData};
use lisk_db::state::state_writer::StateWriter;
use lisk_db::types::CommitOptions;

fn main() -> Result<(), Box<dyn Error>> {
    // thread::sleep(time::Duration::from_secs(20));
    let temp_dir = TempDir::new("bench_db_")?;
    let mut opts = Options::default();
    opts.create_if_missing(true);
    let rocks_db = DB::open(&opts, temp_dir.path())?;
    let (tx, _rx_db) = mpsc::channel::<DbMessage>();
    let common_db = LDB::new(rocks_db, tx, Kind::State);
    let mut db = state_db::StateDB::new(common_db);
    // rx_db.recv_timeout(time::Duration::from_millis(1)).unwrap();
    let mut root = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut root);
    for i in 0..10000 {
        let swriter = StateWriter::default();
        let arc_sw = Arc::new(Mutex::new(swriter));

        for _ in 0..50000 {
            let (tx_snap, rx) = mpsc::channel::<SnapshotMessage>();
            let writer = ReadWriter::new(tx_snap);
            let cloned_arc_sw = Arc::clone(&arc_sw);
            let mut key = [0u8; 32];
            let mut value = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);
            rand::thread_rng().fill_bytes(&mut value);
            writer
                .upsert_key_without_ctx(cloned_arc_sw, key.to_vec(), value.to_vec())
                .unwrap();
            rx.recv_timeout(time::Duration::from_millis(1)).unwrap();
        }

        let op = CommitOptions::new(false, i.into());
        let commit = Commit::new(vec![], op, false);
        let commit_data = CommitData::new(commit, root.to_vec());
        root = db
            .commit_without_ctx(arc_sw, commit_data)
            .unwrap()
            .lock()
            .unwrap()
            .clone()
            .to_vec()
            .try_into()
            .unwrap();

        println!("iteration ({:}) is finished", i);
    }

    println!("Benchmarking successfully completed");

    Ok(())
}
