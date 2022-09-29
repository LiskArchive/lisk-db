use crate::types::{KeyLength, SubtreeHeight, SubtreeHeightKind};

pub const PREFIX_STATE: &[u8] = &[0];
pub const PREFIX_SMT: &[u8] = &[1];
pub const PREFIX_DIFF: &[u8] = &[2];
pub const PREFIX_CURRENT_STATE: &[u8] = &[3];

pub const KEY_LENGTH: KeyLength = KeyLength(38);
pub const SUBTREE_HEIGHT: SubtreeHeight = SubtreeHeight(SubtreeHeightKind::Eight);

pub static PREFIX_BRANCH_HASH: &[u8] = &[1];
