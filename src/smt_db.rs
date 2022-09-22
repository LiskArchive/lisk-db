use crate::consts;
use crate::types::{Cache, KVPair, VecOption, DB};

pub struct SmtDB<'a> {
    db: &'a rocksdb::DB,
    pub batch: rocksdb::WriteBatch,
}

pub struct InMemorySmtDB {
    cache: Cache,
}

impl DB for SmtDB<'_> {
    fn get(&self, key: &[u8]) -> Result<VecOption, rocksdb::Error> {
        let result = self.db.get([consts::PREFIX_SMT, key].concat())?;
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
    pub fn new(db: &'a rocksdb::DB) -> Self {
        Self {
            db,
            batch: rocksdb::WriteBatch::default(),
        }
    }
}

impl DB for InMemorySmtDB {
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

impl InMemorySmtDB {
    pub fn new() -> Self {
        Self {
            cache: Cache::new(),
        }
    }
}
