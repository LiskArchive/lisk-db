/// db_base provides common functionality for Database.
use std::sync::{mpsc, Arc};
use std::thread;

use neon::context::Context;
use neon::event::Channel;
use neon::handle::{Handle, Root};
use neon::types::{Finalize, JsBuffer, JsFunction, JsValue};
use rocksdb::checkpoint::Checkpoint;

use crate::database::traits::{NewDBWithContext, Unwrap};
use crate::database::types::{ArcOptionDB, DbMessage, DbOptions, Kind};

pub struct DB {
    tx: mpsc::Sender<DbMessage>,
    db_kind: Kind,
    db: ArcOptionDB,
}

impl Unwrap for ArcOptionDB {
    fn unwrap(&self) -> &rocksdb::DB {
        self.as_ref().as_ref().expect("The DB connection is None")
    }
}

impl NewDBWithContext for DB {
    fn new_db_with_context<'a, C>(
        ctx: &mut C,
        path: String,
        opts: DbOptions,
        db_kind: Kind,
    ) -> Result<Self, rocksdb::Error>
    where
        C: Context<'a>,
    {
        // Channel for sending callbacks to execute on the sqlite connection thread
        let (tx, rx) = mpsc::channel::<DbMessage>();

        let channel = ctx.channel();

        let mut option = rocksdb::Options::default();
        option.create_if_missing(true);

        let db: rocksdb::DB = if opts.is_readonly() {
            rocksdb::DB::open_for_read_only(&option, path, false)?
        } else {
            rocksdb::DB::open(&option, path)?
        };

        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                match message {
                    DbMessage::Callback(f) => {
                        f(&channel);
                    },
                    DbMessage::Close => return,
                }
            }
        });

        Ok(Self::new(db, tx, db_kind))
    }
}

impl Finalize for DB {}
impl DB {
    fn db(&self) -> &rocksdb::DB {
        self.db.unwrap()
    }

    pub fn new(db: rocksdb::DB, tx: mpsc::Sender<DbMessage>, db_kind: Kind) -> Self {
        Self {
            tx,
            db_kind,
            db: Arc::new(Some(db)),
        }
    }

    // Idiomatic rust would take an owned `self` to prevent use after close
    // However, it's not possible to prevent JavaScript from continuing to hold a closed database
    pub fn close(&mut self) -> Result<(), mpsc::SendError<DbMessage>> {
        self.db = Arc::new(None);
        self.tx.send(DbMessage::Close)
    }

    pub fn send(
        &self,
        callback: impl FnOnce(&Channel) + Send + 'static,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        self.tx.send(DbMessage::Callback(Box::new(callback)))
    }

    pub fn get_by_key(
        &self,
        key: Vec<u8>,
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let key = self.db_kind.key(key);
        let result = self.get(&key);
        self.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(Some(val)) => {
                        let buffer = JsBuffer::external(&mut ctx, val);
                        vec![ctx.null().upcast(), buffer.upcast()]
                    },
                    Ok(None) => vec![ctx.error("No data")?.upcast()],
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    pub fn exists(
        &self,
        key: Vec<u8>,
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let key = self.db_kind.key(key);
        let result = if self.db().key_may_exist(&key) {
            self.get(&key).map(|res| res.is_some())
        } else {
            Ok(false)
        };
        self.send(move |channel| {
            channel.send(move |mut ctx| {
                let callback = callback.into_inner(&mut ctx);
                let this = ctx.undefined();
                let args: Vec<Handle<JsValue>> = match result {
                    Ok(val) => {
                        let converted = ctx.boolean(val);
                        vec![ctx.null().upcast(), converted.upcast()]
                    },
                    Err(err) => vec![ctx.error(&err)?.upcast()],
                };

                callback.call(&mut ctx, this, args)?;

                Ok(())
            });
        })
    }

    pub fn checkpoint(
        &self,
        path: String,
        callback: Root<JsFunction>,
    ) -> Result<(), mpsc::SendError<DbMessage>> {
        let conn = Arc::clone(&self.db);
        self.send(move |channel| {
            let result = Checkpoint::new(conn.unwrap());

            if result.is_err() {
                let err = result.err().unwrap();
                channel.send(move |mut ctx| {
                    let callback = callback.into_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args = vec![ctx.error(&err)?.upcast()];

                    callback.call(&mut ctx, this, args)?;

                    Ok(())
                });
            } else if let Ok(checkpoint) = result {
                let result = checkpoint.create_checkpoint(&path);

                channel.send(move |mut ctx| {
                    let callback = callback.into_inner(&mut ctx);
                    let this = ctx.undefined();
                    let args: Vec<Handle<JsValue>> = match result {
                        Ok(()) => {
                            vec![ctx.null().upcast()]
                        },
                        Err(err) => vec![ctx.error(&err)?.upcast()],
                    };

                    callback.call(&mut ctx, this, args)?;

                    Ok(())
                });
            }
        })
    }

    pub fn arc_clone(&self) -> ArcOptionDB {
        Arc::clone(&self.db)
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), rocksdb::Error> {
        self.db().put(key, value)
    }

    pub fn delete(&self, key: &[u8]) -> Result<(), rocksdb::Error> {
        self.db().delete(key)
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, rocksdb::Error> {
        self.db().get(key)
    }

    pub fn write(&self, batch: rocksdb::WriteBatch) -> Result<(), rocksdb::Error> {
        self.db().write(batch)
    }

    pub fn path(&self) -> &std::path::Path {
        self.db().path()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use tempdir::TempDir;

    use super::*;
    use crate::types::KVPair;

    fn temp_db() -> DB {
        let temp_dir = TempDir::new("test_db").unwrap();
        let rocks_db = rocksdb::DB::open_default(&temp_dir).unwrap();
        let (tx, _) = mpsc::channel::<DbMessage>();
        DB::new(rocks_db, tx, Kind::Normal)
    }

    #[test]
    fn test_put_get_delete() {
        let db = temp_db();
        let key = &[1, 2, 3, 4];
        let value = &[5, 6, 7, 8];
        db.put(key, value).unwrap();
        assert_eq!(db.get(key).unwrap().unwrap(), value);

        db.delete(key).unwrap();
        assert_eq!(db.get(key).unwrap(), None);
    }

    #[test]
    fn test_write_batch() {
        let db = temp_db();
        let pairs: Vec<KVPair> = vec![
            KVPair(vec![], vec![]),
            KVPair(vec![1, 2, 3, 4], vec![4, 5, 6, 7]),
            KVPair(vec![1, 1, 2, 3], vec![5, 8, 13, 21]),
        ];
        let mut batch = rocksdb::WriteBatch::default();
        for pair in &pairs {
            batch.put(pair.key(), pair.value());
        }
        db.write(batch).unwrap();
        for pair in pairs {
            assert_eq!(db.get(pair.key()).unwrap().unwrap(), pair.value());
        }
    }
}
