/// db_base provides common functionality for Database.
use std::sync::{Arc, Mutex};

use neon::types::{Finalize};
use thiserror::Error;

use crate::database::types::{Kind};

pub struct DB {
    db_kind: Kind,
    db: Arc<Mutex<rocksdb::DB>>,
}

pub struct TestIterator<'a> {
    pub db: Arc<Mutex<rocksdb::DB>>,
    pub iterator: Arc<Mutex<rocksdb::DBIterator<'a>>>,
}

impl Finalize for TestIterator<'_> {}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum DBError {
    #[error("Data not found")]
    NotFound,
    #[error("Fail to lock the DB")]
    LockError,
    #[error("RocksDB error: %s")]
    RocksDBError(String),
}

impl Finalize for DB {}
impl DB {
    pub fn new(db: rocksdb::DB, db_kind: Kind) -> Self {
        Self {
            db_kind,
            db: Arc::new(Mutex::new(db)),
        }
    }

    pub fn arc_clone(&self) -> Arc<Mutex<rocksdb::DB>> {
        Arc::clone(&self.db)
    }

    pub fn key(&self, key: Vec<u8>) -> Vec<u8> {
        self.db_kind.key(key)
    }

    // pub fn get_by_key(
    //     &self,
    //     key: Vec<u8>,
    // ) -> Result<Vec<u8>, DBError> {
    //     let locked = self.db.try_lock().map_err(|_| DBError::LockError)?;
    //     let key = ;
    //     let result = locked.get(&key).map_err(|e| DBError::RocksDBError(e.to_string()))?;
    //     result.ok_or(DBError::NotFound)
    // }

    // pub fn exists(
    //     &self,
    //     key: Vec<u8>,
    //     callback: Root<JsFunction>,
    // ) -> Result<(), mpsc::SendError<DbMessage>> {
    //     let key = self.db_kind.key(key);
    //     let result = if self.db().key_may_exist(&key) {
    //         self.get(&key).map(|res| res.is_some())
    //     } else {
    //         Ok(false)
    //     };
    //     self.send(move |channel| {
    //         channel.send(move |mut ctx| {
    //             let callback = callback.into_inner(&mut ctx);
    //             let this = ctx.undefined();
    //             let args: Vec<Handle<JsValue>> = match result {
    //                 Ok(val) => {
    //                     let converted = ctx.boolean(val);
    //                     vec![ctx.null().upcast(), converted.upcast()]
    //                 },
    //                 Err(err) => vec![ctx.error(&err)?.upcast()],
    //             };

    //             callback.call(&mut ctx, this, args)?;

    //             Ok(())
    //         });
    //     })
    // }

    // pub fn checkpoint(
    //     &self,
    //     path: String,
    //     callback: Root<JsFunction>,
    // ) -> Result<(), mpsc::SendError<DbMessage>> {
    //     let conn = Arc::clone(&self.db);
    //     self.send(move |channel| {
    //         let result = Checkpoint::new(conn.unwrap());

    //         if result.is_err() {
    //             let err = result.err().unwrap();
    //             channel.send(move |mut ctx| {
    //                 let callback = callback.into_inner(&mut ctx);
    //                 let this = ctx.undefined();
    //                 let args = vec![ctx.error(&err)?.upcast()];

    //                 callback.call(&mut ctx, this, args)?;

    //                 Ok(())
    //             });
    //         } else if let Ok(checkpoint) = result {
    //             let result = checkpoint.create_checkpoint(&path);

    //             channel.send(move |mut ctx| {
    //                 let callback = callback.into_inner(&mut ctx);
    //                 let this = ctx.undefined();
    //                 let args: Vec<Handle<JsValue>> = match result {
    //                     Ok(()) => {
    //                         vec![ctx.null().upcast()]
    //                     },
    //                     Err(err) => vec![ctx.error(&err)?.upcast()],
    //                 };

    //                 callback.call(&mut ctx, this, args)?;

    //                 Ok(())
    //             });
    //         }
    //     })
    // }

    // pub fn arc_clone(&self) -> ArcOptionDB {
    //     Arc::clone(&self.db)
    // }

    // pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), rocksdb::Error> {
    //     self.db().put(key, value)
    // }

    // pub fn delete(&self, key: &[u8]) -> Result<(), rocksdb::Error> {
    //     self.db().delete(key)
    // }

    // pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, rocksdb::Error> {
    //     self.db().get(key)
    // }

    // pub fn write(&self, batch: rocksdb::WriteBatch) -> Result<(), rocksdb::Error> {
    //     self.db().write(batch)
    // }

    // pub fn path(&self) -> &std::path::Path {
    //     self.db().path()
    // }
}

#[cfg(test)]
mod tests {
    // use std::sync::mpsc;
    // use tempdir::TempDir;

    // use super::*;
    // use crate::types::KVPair;

    // fn temp_db() -> DB {
    //     let temp_dir = TempDir::new("test_db").unwrap();
    //     let rocks_db = rocksdb::DB::open_default(&temp_dir).unwrap();
    //     let (tx, _) = mpsc::channel::<DbMessage>();
    //     DB::new(rocks_db, tx, Kind::Normal)
    // }

    // #[test]
    // fn test_put_get_delete() {
    //     let db = temp_db();
    //     let key = &[1, 2, 3, 4];
    //     let value = &[5, 6, 7, 8];
    //     db.put(key, value).unwrap();
    //     assert_eq!(db.get(key).unwrap().unwrap(), value);

    //     db.delete(key).unwrap();
    //     assert_eq!(db.get(key).unwrap(), None);
    // }

    // #[test]
    // fn test_write_batch() {
    //     let db = temp_db();
    //     let pairs: Vec<KVPair> = vec![
    //         KVPair(vec![], vec![]),
    //         KVPair(vec![1, 2, 3, 4], vec![4, 5, 6, 7]),
    //         KVPair(vec![1, 1, 2, 3], vec![5, 8, 13, 21]),
    //     ];
    //     let mut batch = rocksdb::WriteBatch::default();
    //     for pair in &pairs {
    //         batch.put(pair.key(), pair.value());
    //     }
    //     db.write(batch).unwrap();
    //     for pair in pairs {
    //         assert_eq!(db.get(pair.key()).unwrap().unwrap(), pair.value());
    //     }
    // }
}
