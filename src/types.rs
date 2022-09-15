use std::collections::HashMap;

use crate::codec;

pub type NestedVec = Vec<Vec<u8>>;
pub type Cache = HashMap<Vec<u8>, Vec<u8>>;
pub type VecOption = Option<Vec<u8>>;

#[derive(Clone, Debug)]
pub struct KVPair(pub Vec<u8>, pub Vec<u8>);

pub trait DB {
    fn get(&self, key: &[u8]) -> Result<VecOption, rocksdb::Error>;
    fn set(&mut self, pair: &KVPair) -> Result<(), rocksdb::Error>;
    fn del(&mut self, key: &[u8]) -> Result<(), rocksdb::Error>;
}

pub trait New {
    fn new() -> Self;
}

pub trait KVPairCodec {
    fn decode(val: &[u8]) -> Result<KVPair, codec::CodecError>;
    fn encode(&self) -> Vec<u8>;
}

impl New for Cache {
    fn new() -> Self {
        HashMap::new()
    }
}

impl New for NestedVec {
    fn new() -> Self {
        vec![]
    }
}

impl KVPair {
    pub fn new(key: &[u8], value: &[u8]) -> Self {
        Self(key.to_vec(), value.to_vec())
    }

    pub fn key(&self) -> &[u8] {
        &self.0
    }

    pub fn value(&self) -> &[u8] {
        &self.1
    }

    pub fn key_as_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn value_as_vec(&self) -> Vec<u8> {
        self.1.to_vec()
    }

    pub fn is_empty_value(&self) -> bool {
        self.1.is_empty()
    }
}
