use std::collections::HashMap;

use crate::consts;
use crate::smt;

pub struct SMTDB<'a> {
    db: Box<&'a rocksdb::DB>,
    pub batch: rocksdb::WriteBatch,
}

impl<'a> SMTDB<'a> {
    pub fn new(db: &'a rocksdb::DB) -> Self {
        let b = Box::new(db);
        Self {
            db: b,
            batch: rocksdb::WriteBatch::default(),
        }
    }
}

impl smt::DB for SMTDB<'_> {
    fn get(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, rocksdb::Error> {
        let result = self.db.get([consts::PREFIX_SMT, key.as_slice()].concat())?;
        Ok(result)
    }

    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), rocksdb::Error> {
        self.batch.put(key, value);
        Ok(())
    }

    fn del(&mut self, key: Vec<u8>) -> Result<(), rocksdb::Error> {
        self.batch.delete(key);
        Ok(())
    }
}

pub struct InMemorySMTDB {
    cache: HashMap<Vec<u8>, Vec<u8>>,
}

impl InMemorySMTDB {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }
}

impl smt::DB for InMemorySMTDB {
    fn get(&self, key: Vec<u8>) -> Result<Option<Vec<u8>>, rocksdb::Error> {
        let result = self.cache.get(&key);
        if let Some(value) = result {
            return Ok(Some(value.clone()));
        }
        Ok(None)
    }

    fn set(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), rocksdb::Error> {
        self.cache.insert(key, value);
        Ok(())
    }

    fn del(&mut self, key: Vec<u8>) -> Result<(), rocksdb::Error> {
        self.cache.remove(&key);
        Ok(())
    }
}
