use std::collections::HashMap;
use std::ops::{Add, Sub};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::codec;
use crate::consts::PREFIX_BRANCH_HASH;

const PREFIX_SIZE: usize = 6;

pub type NestedVecGeneric<T> = Vec<Vec<T>>;
pub type NestedVec = NestedVecGeneric<u8>;
pub type NestedVecOfSlices<'a> = NestedVecGeneric<&'a [u8]>;
pub type SharedNestedVec<'a> = Vec<&'a [u8]>;
pub type Cache = HashMap<Vec<u8>, Vec<u8>>;
pub type VecOption = Option<Vec<u8>>;
pub type SharedVec = Arc<Mutex<Arc<Vec<u8>>>>;
pub type ArcMutex<T> = Arc<Mutex<T>>;
pub type CommitOptions = Options<BlockHeight>;

// Strong type of SMT with max value KEY_LENGTH * 8
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct Height(pub u16);

// Strong type of block height
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct BlockHeight(pub u32);

// Strong type of structure position in Subtree with max value 2 ^ SUBTREE_SIZE
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub struct StructurePosition(pub u16);

// Strong type of subtree height with values of SubtreeHeightKind
#[derive(Clone, Debug, Copy)]
pub struct SubtreeHeight(pub SubtreeHeightKind);

#[derive(Clone, Debug, Copy)]
pub struct KeyLength(pub u16);

// Options is a base class for type CommitOptions and DbOptions
#[derive(Debug, Copy, Clone)]
pub struct Options<T> {
    readonly: bool,
    pub number: T,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum SubtreeHeightKind {
    Four = 4,
    Eight = 8,
    Sixteen = 16,
}

// HashKind represents kind of Vec that should be used in HashWithKind trait
#[derive(PartialEq, Eq)]
pub enum HashKind {
    Key,
    Value,
    Branch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KVPair(pub Vec<u8>, pub Vec<u8>);

#[derive(Clone, Debug)]
pub struct SharedKVPair<'a>(pub &'a [u8], pub &'a [u8]);

pub trait New {
    fn new() -> Self;
}

pub trait Hash256 {
    fn hash(&self) -> Vec<u8>;
}

pub trait HashWithKind {
    fn hash_with_kind(&self, kind: HashKind) -> Vec<u8>;
}

pub trait KVPairCodec {
    fn decode(val: &[u8]) -> Result<KVPair, codec::CodecError>;
    fn encode(&self) -> Vec<u8>;
}

impl New for Cache {
    #[inline]
    fn new() -> Self {
        HashMap::new()
    }
}

impl New for NestedVec {
    #[inline]
    fn new() -> Self {
        vec![]
    }
}

impl From<&u8> for Height {
    #[inline]
    fn from(value: &u8) -> Height {
        Height(*value as u16)
    }
}

impl From<u8> for Height {
    #[inline]
    fn from(value: u8) -> Height {
        Height(value as u16)
    }
}

impl From<Height> for u8 {
    #[inline]
    fn from(value: Height) -> u8 {
        value.0 as u8
    }
}

impl From<Height> for usize {
    #[inline]
    fn from(value: Height) -> usize {
        value.0 as usize
    }
}

impl Add for Height {
    type Output = Self;
    #[inline]
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Height {
    type Output = Self;
    #[inline]
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl From<KeyLength> for u16 {
    #[inline]
    fn from(value: KeyLength) -> u16 {
        value.0
    }
}

impl From<usize> for KeyLength {
    #[inline]
    fn from(value: usize) -> KeyLength {
        Self(value as u16)
    }
}

impl From<KeyLength> for usize {
    #[inline]
    fn from(value: KeyLength) -> usize {
        value.0 as usize
    }
}

impl From<f64> for KeyLength {
    #[inline]
    fn from(value: f64) -> KeyLength {
        Self(value as u16)
    }
}

impl From<KeyLength> for BlockHeight {
    #[inline]
    fn from(value: KeyLength) -> BlockHeight {
        Self(value.0 as u32)
    }
}

impl From<u32> for BlockHeight {
    #[inline]
    fn from(value: u32) -> BlockHeight {
        BlockHeight(value)
    }
}

impl From<f64> for BlockHeight {
    #[inline]
    fn from(value: f64) -> BlockHeight {
        BlockHeight(value as u32)
    }
}

impl Sub for BlockHeight {
    type Output = Self;
    #[inline]
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl From<BlockHeight> for u32 {
    #[inline]
    fn from(value: BlockHeight) -> u32 {
        value.0
    }
}

impl From<BlockHeight> for usize {
    #[inline]
    fn from(value: BlockHeight) -> usize {
        value.0 as usize
    }
}

impl Default for SubtreeHeight {
    #[inline]
    fn default() -> Self {
        SubtreeHeight(SubtreeHeightKind::Four)
    }
}

impl Add for StructurePosition {
    type Output = Self;
    #[inline]
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl From<SubtreeHeight> for StructurePosition {
    #[inline]
    fn from(value: SubtreeHeight) -> StructurePosition {
        StructurePosition(value.u16())
    }
}

impl From<StructurePosition> for u8 {
    #[inline]
    fn from(value: StructurePosition) -> u8 {
        value.0 as u8
    }
}

impl From<u8> for StructurePosition {
    #[inline]
    fn from(value: u8) -> StructurePosition {
        StructurePosition(value as u16)
    }
}

impl From<StructurePosition> for Height {
    #[inline]
    fn from(value: StructurePosition) -> Height {
        Height(value.0)
    }
}

impl HashWithKind for Vec<u8> {
    fn hash_with_kind(&self, kind: HashKind) -> Vec<u8> {
        let mut hasher = Sha256::new();
        match kind {
            HashKind::Key => {
                let body = &self[PREFIX_SIZE..];
                hasher.update(body);
            },
            HashKind::Value => {
                hasher.update(self);
            },
            HashKind::Branch => {
                hasher.update(PREFIX_BRANCH_HASH);
                hasher.update(self);
            },
        };
        let result = hasher.finalize();
        if kind == HashKind::Key {
            let prefix = &self[..PREFIX_SIZE];
            [prefix, result.as_slice()].concat()
        } else {
            result.to_vec()
        }
    }
}

impl<T> Options<T> {
    #[inline]
    pub fn new(readonly: bool, number: T) -> Self {
        Self { readonly, number }
    }

    #[inline]
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }
}

impl CommitOptions {
    #[inline]
    pub fn version(&self) -> BlockHeight {
        self.number
    }
}

impl KVPair {
    #[inline]
    pub fn new(key: &[u8], value: &[u8]) -> Self {
        Self(key.to_vec(), value.to_vec())
    }

    #[inline]
    pub fn key(&self) -> &[u8] {
        &self.0
    }

    #[inline]
    pub fn value(&self) -> &[u8] {
        &self.1
    }

    #[inline]
    pub fn key_as_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    #[inline]
    pub fn value_as_vec(&self) -> Vec<u8> {
        self.1.to_vec()
    }

    #[inline]
    pub fn is_empty_value(&self) -> bool {
        self.1.is_empty()
    }
}

impl<'a> SharedKVPair<'a> {
    #[inline]
    pub fn new(key: &'a [u8], value: &'a [u8]) -> Self {
        Self(key, value)
    }

    #[inline]
    pub fn key(&self) -> &[u8] {
        self.0
    }

    #[inline]
    pub fn value(&self) -> &[u8] {
        self.1
    }

    #[inline]
    pub fn key_as_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    #[inline]
    pub fn value_as_vec(&self) -> Vec<u8> {
        self.1.to_vec()
    }
}

impl Height {
    #[inline]
    pub fn is_equal_to(self, value: u16) -> bool {
        self.0 == value
    }

    #[inline]
    pub fn div_to_usize(self, value: u16) -> usize {
        (self.0 / value) as usize
    }

    #[inline]
    pub fn mod_to_u8(self, value: u16) -> u8 {
        (self.0 % value) as u8
    }
}

impl BlockHeight {
    #[inline]
    pub fn to_be_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    #[inline]
    pub fn is_equal_to(self, value: u32) -> bool {
        self.0 == value
    }
}

impl KeyLength {
    // Cast to u32 and returns with len(4) for JS API
    #[inline]
    pub fn as_u32_to_be_bytes(self) -> [u8; 4] {
        (self.0 as u32).to_be_bytes()
    }
}

impl SubtreeHeight {
    #[inline]
    pub fn u16(self) -> u16 {
        self.0 as u16
    }

    #[inline]
    pub fn is_four(self) -> bool {
        self.0 == SubtreeHeightKind::Four
    }

    #[inline]
    pub fn sub_to_usize(self, value: u8) -> usize {
        (self.u16() - value as u16) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_with_kind() {
        let data = vec![
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29,
            0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
            0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
        ];

        let hash = data.hash_with_kind(HashKind::Key);
        assert_eq!(
            hash,
            vec![
                0, 1, 2, 3, 4, 5, 10, 165, 4, 79, 85, 150, 61, 222, 215, 50, 50, 134, 228, 240,
                180, 58, 75, 167, 243, 47, 247, 126, 21, 28, 34, 193, 134, 242, 138, 162, 150, 75
            ]
        );

        let hash = data.hash_with_kind(HashKind::Value);
        assert_eq!(
            hash,
            vec![
                253, 234, 185, 172, 243, 113, 3, 98, 189, 38, 88, 205, 201, 162, 158, 143, 156,
                117, 127, 207, 152, 17, 96, 58, 140, 68, 124, 209, 217, 21, 17, 8
            ]
        );

        let hash = data.hash_with_kind(HashKind::Branch);
        assert_eq!(
            hash,
            vec![
                124, 122, 21, 79, 68, 38, 216, 75, 248, 234, 239, 61, 253, 235, 23, 34, 109, 82,
                4, 50, 149, 247, 207, 222, 25, 26, 24, 91, 100, 19, 174, 131
            ]
        );
    }

    #[test]
    fn test_values_subtree_height_kind() {
        let test_data = vec![
            (SubtreeHeightKind::Four, 4u16),
            (SubtreeHeightKind::Eight, 8u16),
            (SubtreeHeightKind::Sixteen, 16u16),
        ];
        for (data, result) in test_data {
            assert_eq!(SubtreeHeight(data).u16(), result);
        }
    }
}
