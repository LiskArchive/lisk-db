use crate::types::{KeyLength, SubtreeHeight, SubtreeHeightKind};

/// KEY_LENGTH is default key length for state_db.
pub const KEY_LENGTH: KeyLength = KeyLength(38);
/// SUBTREE_HEIGHT is default subtree height for state_db.
pub const SUBTREE_HEIGHT: SubtreeHeight = SubtreeHeight(SubtreeHeightKind::Four);

/// PREFIX_LEAF_HASH is prefix for creating leaf node hash.
pub static PREFIX_LEAF_HASH: &[u8] = "LSK_SMTL_".as_bytes();
/// PREFIX_BRANCH_HASH is prefix for creating branch node hash.
pub static PREFIX_BRANCH_HASH: &[u8] = "LSK_SMTB_".as_bytes();
/// PREFIX_EMPTY is prefix for creating empty node hash.
pub static PREFIX_EMPTY: &[u8] = &[2];

/// Prefix is the database prefix to separate the keys in the state_db.
pub struct Prefix;
impl Prefix {
    /// STATE maintains latest state of the state_db.
    pub const STATE: &'static [u8] = &[0];
    /// SMT maintains sparse merkle tree hashes.
    pub const SMT: &'static [u8] = &[1];
    /// DIFF maintains state difference between last state update.
    pub const DIFF: &'static [u8] = &[2];
    /// CURRENT_STATE maintains current version and the root hash of the state_db.
    pub const CURRENT_STATE: &'static [u8] = &[3];
}
