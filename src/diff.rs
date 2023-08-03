/// diff provides data structure to revert the state for StateDB.
use crate::batch;
use crate::codec;
use crate::types::{Cache, HashKind, HashWithKind, KVPair, KVPairCodec, NestedVec};

/// Diff maintains difference between each state changes, and it is used when reverting the state.
/// When updating state to next state, it maintains:
/// - newly created keys.
/// - updated keys and corresponding original values
/// - deleted keys and corresponding original values
/// When reverting the state,
/// - Remove created keys
/// - Update updated to the value
/// - Create deleted key with the value
#[derive(Clone, Debug, PartialEq, Eq)]
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
        writer.result().to_vec()
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
    /// decode bytes to diff struct.
    /// decoding uses lisk-codec protocol.
    pub fn decode(val: &[u8]) -> Result<Self, codec::CodecError> {
        let mut reader = codec::Reader::new(val);
        let created = reader.read_bytes_slice(1)?;
        let updated_bytes = reader.read_bytes_slice(2)?;
        let updated: Vec<KVPair> = updated_bytes
            .iter()
            .map(|value| KVPair::decode(value).unwrap())
            .collect();
        let deleted_bytes = reader.read_bytes_slice(3)?;
        let deleted: Vec<KVPair> = deleted_bytes
            .iter()
            .map(|value| KVPair::decode(value).unwrap())
            .collect();
        Ok(Self {
            created,
            updated,
            deleted,
        })
    }

    /// encode diff to bytes.
    /// encoding uses lisk-codec protocol.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = codec::Writer::new();
        writer.write_bytes_slice(1, &self.created);
        let updated: NestedVec = self.updated.iter().map(|v| v.encode()).collect();
        writer.write_bytes_slice(2, &updated);
        let deleted: NestedVec = self.deleted.iter().map(|v| v.encode()).collect();
        writer.write_bytes_slice(3, &deleted);

        writer.result().to_vec()
    }

    /// revert_hashed_update returns cache value with original data.
    /// Deleting data is represented as empty bytes.
    pub fn revert_hashed_update(&self) -> Cache {
        let mut result = Cache::new();
        for kv in self.updated.iter() {
            result.insert(
                kv.key_as_vec().hash_with_kind(HashKind::Key),
                kv.value_as_vec().hash_with_kind(HashKind::Value),
            );
        }
        for kv in self.deleted.iter() {
            result.insert(
                kv.key_as_vec().hash_with_kind(HashKind::Key),
                kv.value_as_vec().hash_with_kind(HashKind::Value),
            );
        }
        for key in self.created.iter() {
            result.insert(key.to_vec().hash_with_kind(HashKind::Key), vec![]);
        }
        result
    }

    /// revert_commit updates batch to revert the states.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts;

    #[test]
    fn test_kvpair_encode_decode() {
        let kvpair = KVPair::new(b"test_key", b"test_value");

        let encoded = kvpair.encode();
        let decoded = KVPair::decode(&encoded).unwrap();

        assert_eq!(kvpair, decoded);
    }

    #[test]
    fn test_diff_new() {
        let created = vec![b"test_key".to_vec()];
        let updated = vec![KVPair::new(b"test_key", b"test_value")];
        let deleted = vec![KVPair::new(b"test_key_deleted", b"test_value_deleted")];

        let diff = Diff::new(created.clone(), updated.clone(), deleted.clone());
        assert_eq!(diff.created, created);
        assert_eq!(diff.updated, updated);
        assert_eq!(diff.deleted, deleted);
    }

    #[test]
    fn test_diff_encode_decode() {
        let created = vec![b"test_key".to_vec()];
        let updated = vec![KVPair::new(b"test_key", b"test_value")];
        let deleted = vec![KVPair::new(b"test_key_deleted", b"test_value_deleted")];
        let diff = Diff::new(created, updated, deleted);

        let encoded = diff.encode();
        let decoded = Diff::decode(&encoded).unwrap();

        assert_eq!(diff, decoded);
    }

    #[test]
    fn test_diff_revert_hashed_update() {
        let created = vec![b"test_key".to_vec()];
        let updated = vec![KVPair::new(b"test_key_updated", b"test_value_updated")];
        let deleted = vec![KVPair::new(b"test_key_deleted", b"test_value_deleted")];
        let diff = Diff::new(created, updated, deleted);

        let cache = diff.revert_hashed_update();

        assert_eq!(
            cache.get(&b"test_key".to_vec().hash_with_kind(HashKind::Key)),
            Some(&vec![])
        );
        assert_eq!(
            cache.get(&b"test_key_updated".to_vec().hash_with_kind(HashKind::Key)),
            Some(
                &b"test_value_updated"
                    .to_vec()
                    .hash_with_kind(HashKind::Value)
            )
        );
        assert_eq!(
            cache.get(&b"test_key_deleted".to_vec().hash_with_kind(HashKind::Key)),
            Some(
                &b"test_value_deleted"
                    .to_vec()
                    .hash_with_kind(HashKind::Value)
            )
        );
    }

    #[test]
    fn test_diff_revert_commit() {
        let created = vec![b"test_key".to_vec()];
        let updated = vec![KVPair::new(b"test_key_updated", b"test_value_updated")];
        let deleted = vec![KVPair::new(b"test_key_deleted", b"test_value_deleted")];
        let diff = Diff::new(created, updated, deleted);

        let mut batch = batch::PrefixWriteBatch::new();
        batch.set_prefix(&consts::Prefix::STATE);

        diff.revert_commit(&mut batch);

        assert_eq!(batch.batch.len(), 3);
    }
}
