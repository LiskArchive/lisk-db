use crate::types::{KeyLength, SubtreeHeight, SubtreeHeightKind};

pub const KEY_LENGTH: KeyLength = KeyLength(38);
pub const SUBTREE_HEIGHT: SubtreeHeight = SubtreeHeight(SubtreeHeightKind::Four);

pub static PREFIX_LEAF_HASH: &[u8] = &[0];
pub static PREFIX_BRANCH_HASH: &[u8] = &[1];
pub static PREFIX_EMPTY: &[u8] = &[2];

pub struct Prefix;
impl Prefix {
    pub const STATE: &'static [u8] = &[0];
    pub const SMT: &'static [u8] = &[1];
    pub const DIFF: &'static [u8] = &[2];
    pub const CURRENT_STATE: &'static [u8] = &[3];
}
