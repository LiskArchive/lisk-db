use std::cmp;
use sha2::{Digest, Sha256};

pub fn empty_hash() -> Vec<u8> {
    let hasher = Sha256::new();
    let result = hasher.finalize();
    return result.as_slice().to_vec();
}

pub fn compare(a: &[u8], b: &[u8]) -> cmp::Ordering {
    for (ai, bi) in a.iter().zip(b.iter()) {
        match ai.cmp(&bi) {
            cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    /* if every single element was equal, compare length */
    a.len().cmp(&b.len())
}

pub fn is_bit_set(bits: &[u8], i: usize) -> bool {
    ((bits[i / 8] << i % 8) & 0x80) == 0x80
}

pub fn is_bytes_equal(a: &Vec<u8>, b: &Vec<u8>) -> bool {
    compare(a, b) == cmp::Ordering::Equal
}

pub fn is_empty_hash(a: &Vec<u8>) -> bool {
    compare(a, empty_hash().as_slice()) == cmp::Ordering::Equal
}