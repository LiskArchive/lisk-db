use std::cell::RefCell;
use std::sync::Arc;

use neon::event::Channel;
use neon::types::JsBox;

use crate::consts::Prefix;
use crate::types::{ArcMutex, KeyLength, Options};

type SnapshotCallback = Box<dyn FnOnce(&rocksdb::Snapshot, &Channel) + Send>;
type DbCallback = Box<dyn FnOnce(&Channel) + Send>;
pub type DbOptions = Options<KeyLength>;

pub type JsBoxRef<T> = JsBox<RefCell<T>>;
pub type JsArcMutex<T> = JsBoxRef<ArcMutex<T>>;
pub type ArcOptionDB = Arc<Option<rocksdb::DB>>;

/// Messages sent on the database channel
pub enum Message<T> {
    /// Callback to be executed
    Callback(T),
    /// Indicates that the thread should be stopped and connection closed
    Close,
}

pub type SnapshotMessage = Message<SnapshotCallback>;
pub type DbMessage = Message<DbCallback>;

/// Kind represented the kind of the database
#[derive(PartialEq, Eq)]
pub enum Kind {
    Normal,
    State,
    StateWriter,
    Batch,
    InMemorySMT,
}

impl DbOptions {
    #[inline]
    pub fn key_length(&self) -> KeyLength {
        self.number
    }
}

impl Kind {
    pub fn key(&self, key: Vec<u8>) -> Vec<u8> {
        match self {
            Kind::State => [Prefix::STATE, &key].concat(),
            _ => key,
        }
    }
}
