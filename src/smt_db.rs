use std::collections::HashMap;

use crate::consts;
use crate::smt;

pub struct SmtDB<'a> {
    db: &'a rocksdb::DB,
    pub batch: rocksdb::WriteBatch,
}

impl<'a> SmtDB<'a> {
    pub fn new(db: &'a rocksdb::DB) -> Self {
        Self {
            db,
            batch: rocksdb::WriteBatch::default(),
        }
    }
}

impl smt::DB for SmtDB<'_> {
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

pub struct InMemorySmtDB {
    cache: HashMap<Vec<u8>, Vec<u8>>,
}

impl InMemorySmtDB {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }
}

impl smt::DB for InMemorySmtDB {
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
