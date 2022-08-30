use std::collections::HashMap;

use crate::batch;
use crate::codec;

#[derive(Clone, Debug)]
pub struct KeyValue {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl KeyValue {
    pub fn new(key: Vec<u8>, value: Vec<u8>) -> Self {
        Self {
            key: key,
            value: value,
        }
    }

    pub fn decode(val: Vec<u8>) -> Result<Self, codec::CodecError> {
        let mut reader = codec::Reader::new(val);
        let key = reader.read_bytes(1)?;
        let value = reader.read_bytes(2)?;
        Ok(Self {
            key: key,
            value: value,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = codec::Writer::new();
        writer.write_bytes(1, &self.key);
        writer.write_bytes(2, &self.value);
        writer.result()
    }
}

#[derive(Clone, Debug)]
pub struct Diff {
    created: Vec<Vec<u8>>,
    updated: Vec<KeyValue>,
    deleted: Vec<KeyValue>,
}

impl Diff {
    pub fn new(created: Vec<Vec<u8>>, updated: Vec<KeyValue>, deleted: Vec<KeyValue>) -> Self {
        Self {
            created: created,
            updated: updated,
            deleted: deleted,
        }
    }

    pub fn decode(val: Vec<u8>) -> Result<Self, codec::CodecError> {
        let mut reader = codec::Reader::new(val);
        let created = reader.read_bytes_slice(1)?;
        let updated_bytes = reader.read_bytes_slice(2)?;
        let mut updated = vec![];
        for value in updated_bytes.iter() {
            let kv = KeyValue::decode(value.to_vec())?;
            updated.push(kv);
        }
        let deleted_bytes = reader.read_bytes_slice(3)?;
        let mut deleted = vec![];
        for value in deleted_bytes.iter() {
            let kv = KeyValue::decode(value.to_vec())?;
            deleted.push(kv);
        }
        Ok(Self {
            created: created,
            updated: updated,
            deleted: deleted,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = codec::Writer::new();
        writer.write_bytes_slice(1, &self.created);
        let updated: Vec<Vec<u8>> = self.updated.iter().map(|v| v.encode()).collect();
        writer.write_bytes_slice(2, &updated);
        let deleted: Vec<Vec<u8>> = self.deleted.iter().map(|v| v.encode()).collect();
        writer.write_bytes_slice(3, &deleted);

        writer.result()
    }

    pub fn revert_update(&self) -> HashMap<Vec<u8>, Vec<u8>> {
        let mut result: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
        for kv in self.updated.iter() {
            result.insert(kv.key.to_vec(), kv.value.to_vec());
        }
        for kv in self.deleted.iter() {
            result.insert(kv.key.to_vec(), kv.value.to_vec());
        }
        for key in self.created.iter() {
            result.insert(key.to_vec(), vec![]);
        }
        result
    }

    pub fn revert_commit(&self, batch: &mut impl batch::BatchWriter) {
        for kv in self.updated.iter() {
            batch.put(&kv.key, &kv.value);
        }
        for kv in self.deleted.iter() {
            batch.put(&kv.key, &kv.value);
        }
        for key in self.created.iter() {
            batch.delete(&key);
        }
    }
}
