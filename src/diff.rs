use crate::batch;
use crate::codec;
use crate::types::{Cache, KVPair, KVPairCodec, NestedVec};

#[derive(Clone, Debug)]
pub struct Diff {
    created: NestedVec,
    updated: Vec<KVPair>,
    deleted: Vec<KVPair>,
}

impl KVPairCodec for KVPair {
    fn decode(val: &[u8]) -> Result<Self, codec::CodecError> {
        let mut reader = codec::Reader::new(val);
        let key = reader.read_bytes(1)?;
        let value = reader.read_bytes(2)?;
        Ok(Self::new(&key, &value))
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = codec::Writer::new();
        writer.write_bytes(1, self.key());
        writer.write_bytes(2, self.value());
        writer.result()
    }
}

impl Diff {
    pub fn new(created: NestedVec, updated: Vec<KVPair>, deleted: Vec<KVPair>) -> Self {
        Self {
            created,
            updated,
            deleted,
        }
    }

    pub fn decode(val: &[u8]) -> Result<Self, codec::CodecError> {
        let mut reader = codec::Reader::new(val);
        let created = reader.read_bytes_slice(1)?;
        let updated_bytes = reader.read_bytes_slice(2)?;
        let mut updated = vec![];
        for value in updated_bytes.iter() {
            let kv = KVPair::decode(value)?;
            updated.push(kv);
        }
        let deleted_bytes = reader.read_bytes_slice(3)?;
        let mut deleted = vec![];
        for value in deleted_bytes.iter() {
            let kv = KVPair::decode(value)?;
            deleted.push(kv);
        }
        Ok(Self {
            created,
            updated,
            deleted,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = codec::Writer::new();
        writer.write_bytes_slice(1, &self.created);
        let updated: NestedVec = self.updated.iter().map(|v| v.encode()).collect();
        writer.write_bytes_slice(2, &updated);
        let deleted: NestedVec = self.deleted.iter().map(|v| v.encode()).collect();
        writer.write_bytes_slice(3, &deleted);

        writer.result()
    }

    pub fn revert_update(&self) -> Cache {
        let mut result = Cache::new();
        for kv in self.updated.iter() {
            result.insert(kv.key_as_vec(), kv.value_as_vec());
        }
        for kv in self.deleted.iter() {
            result.insert(kv.key_as_vec(), kv.value_as_vec());
        }
        for key in self.created.iter() {
            result.insert(key.to_vec(), vec![]);
        }
        result
    }

    pub fn revert_commit(&self, batch: &mut impl batch::BatchWriter) {
        for kv in self.updated.iter() {
            batch.put(kv);
        }
        for kv in self.deleted.iter() {
            batch.put(kv);
        }
        for key in self.created.iter() {
            batch.delete(key);
        }
    }
}
