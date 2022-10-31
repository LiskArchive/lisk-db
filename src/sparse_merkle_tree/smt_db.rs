// smt_db provides in memory interface for in memory SMT computation.
use crate::consts;
use crate::database::traits::Actions;
use crate::database::DB;
use crate::types::{Cache, KVPair, VecOption};

pub struct SmtDB<'a> {
    db: &'a DB,
    pub batch: rocksdb::WriteBatch,
}

#[derive(Default)]
pub struct InMemorySmtDB {
    cache: Cache,
}

impl Actions for SmtDB<'_> {
    fn get(&self, key: &[u8]) -> Result<VecOption, rocksdb::Error> {
        let result = self.db.get(&[consts::Prefix::SMT, key].concat())?;
        Ok(result)
    }

    fn set(&mut self, pair: &KVPair) -> Result<(), rocksdb::Error> {
        self.batch.put(pair.key(), pair.value());
        Ok(())
    }

    fn del(&mut self, key: &[u8]) -> Result<(), rocksdb::Error> {
        self.batch.delete(key);
        Ok(())
    }
}

impl<'a> SmtDB<'a> {
    pub fn new(db: &'a DB) -> Self {
        Self {
            db,
            batch: rocksdb::WriteBatch::default(),
        }
    }
}

impl Actions for InMemorySmtDB {
    fn get(&self, key: &[u8]) -> Result<VecOption, rocksdb::Error> {
        let result = self.cache.get(key);
        if let Some(value) = result {
            return Ok(Some(value.clone()));
        }
        Ok(None)
    }

    fn set(&mut self, pair: &KVPair) -> Result<(), rocksdb::Error> {
        self.cache.insert(pair.key_as_vec(), pair.value_as_vec());
        Ok(())
    }

    fn del(&mut self, key: &[u8]) -> Result<(), rocksdb::Error> {
        self.cache.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use tempdir::TempDir;

    use crate::batch::PrefixWriteBatch;
    use crate::database::types::{DbMessage, Kind};
    use crate::database::DB;

    use super::*;

    fn temp_db() -> (DB, TempDir) {
        let temp_dir = TempDir::new("test_smt_db").unwrap();
        let rocks_db = rocksdb::DB::open_default(&temp_dir).unwrap();
        let (tx, _) = mpsc::channel::<DbMessage>();
        (DB::new(rocks_db, tx, Kind::Normal), temp_dir)
    }

    #[test]
    fn test_new_smt_db() {
        let (db, temp_dir) = temp_db();
        let smt_db = SmtDB::new(&db);

        assert_eq!(
            smt_db.db.path().to_str().unwrap(),
            temp_dir.path().to_str().unwrap()
        );
    }

    #[test]
    fn test_smt_db_get() {
        let (db, _) = temp_db();
        let mut smt_db = SmtDB::new(&db);

        smt_db
            .set(&KVPair::new(b"test_key", b"test_value"))
            .unwrap();
        assert_eq!(smt_db.batch.len(), 1);

        let mut write_batch = PrefixWriteBatch::new();
        write_batch.set_prefix(&consts::Prefix::SMT);
        smt_db.batch.iterate(&mut write_batch);
        smt_db.db.write(write_batch.batch).unwrap();

        let value = smt_db.get(b"test_key").unwrap();
        assert_eq!(value, Some(b"test_value".to_vec()));
    }

    #[test]
    fn test_smt_db_set() {
        let (db, _) = temp_db();
        let mut smt_db = SmtDB::new(&db);

        smt_db
            .set(&KVPair::new(b"test_key", b"test_value"))
            .unwrap();
        assert_eq!(smt_db.batch.len(), 1);
    }

    #[test]
    fn test_smt_db_del() {
        let (db, _) = temp_db();
        let mut smt_db = SmtDB::new(&db);

        smt_db
            .set(&KVPair::new(b"test_key", b"test_value"))
            .unwrap();
        assert_eq!(smt_db.batch.len(), 1);

        smt_db.del(b"test_key").unwrap();
        assert_eq!(smt_db.batch.len(), 2);
    }

    #[test]
    fn test_in_memory_smt_db_get() {
        let mut db = InMemorySmtDB::default();

        db.set(&KVPair::new(b"test_key", b"test_value")).unwrap();

        let result = db.get(b"test_key").unwrap();
        assert_eq!(result, Some(b"test_value".to_vec()));
    }

    #[test]
    fn test_in_memory_smt_db_get_none() {
        let mut db = InMemorySmtDB::default();

        db.set(&KVPair::new(b"test_key", b"test_value")).unwrap();

        let result = db.get(b"test_key_not_existing").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_in_memory_smt_db_del() {
        let mut db = InMemorySmtDB::default();

        db.set(&KVPair::new(b"test_key", b"test_value")).unwrap();
        db.del(b"test_key").unwrap();

        let result = db.get(b"test_key").unwrap();
        assert_eq!(result, None);
    }
}
