use sha2::{Digest, Sha256};
use std::cmp;

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

pub fn is_bytes_equal(a: &[u8], b: &[u8]) -> bool {
    compare(a, b) == cmp::Ordering::Equal
}

pub fn is_empty_hash(a: &Vec<u8>) -> bool {
    compare(a, empty_hash().as_slice()) == cmp::Ordering::Equal
}

pub fn is_bools_equal(a: &[bool], b: &[bool]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (i, val) in a.iter().enumerate() {
        if *val != b[i] {
            return false;
        }
    }
    true
}

pub fn bools_to_bytes(a: &[bool]) -> Vec<u8> {
    let mut result = vec![0; (a.len() + 7) / 8];
    let mut missing_byte = 0;
    if a.len() % 8 != 0 {
        missing_byte = 8 - a.len() % 8;
    }
    let mut target = vec![];
    for _ in 0..missing_byte {
        target.push(false);
    }
    target.extend(a);

    for (i, v) in target.iter().enumerate() {
        if *v {
            result[i / 8] |= 0x80 >> (i % 8);
        }
    }
    result
}

pub fn bytes_to_bools(a: &[u8]) -> Vec<bool> {
    let mut result = vec![false; a.len() * 8];
    for (i, x) in a.iter().enumerate() {
        for j in 0..8 {
            result[8 * i + j] = (x << j) & 0x80 == 0x80;
        }
    }
    result
}

pub fn common_prefix(a: &[bool], b: &[bool]) -> Vec<bool> {
    let mut result = vec![];
    let mut longer = a;
    let mut shorter = b;
    if longer.len() < shorter.len() {
        let tmp = longer;
        longer = shorter;
        shorter = tmp;
    }
    for (i, v) in longer.iter().enumerate() {
        if i >= shorter.len() {
            return result;
        }
        if *v == shorter[i] {
            result.push(*v);
        }
    }
    result
}

pub fn strip_left_false(a: &[bool]) -> Vec<bool> {
    let mut result = vec![];
    let mut saw_true = false;
    for v in a {
        if saw_true || *v {
            saw_true = true;
            result.push(*v);
        }
    }
    result
}

pub fn bytes_in(list: &Vec<Vec<u8>>, a: &[u8]) -> bool {
    for v in list {
        if is_bytes_equal(v, a) {
            return true;
        }
    }
    false
}

pub fn arr_eq_bool(a: &[bool], b: &[bool]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (i, _) in a.iter().enumerate() {
        if a[i] != b[i] {
            return false;
        }
    }
    true
}

pub fn binary_search<T>(list: &[T], cb: impl Fn(&T) -> bool) -> i32 {
    let mut lo = -1;
    let mut hi = list.len() as i32;
    while 1 + lo < hi {
        let mi = lo + ((hi - lo) >> 1);
        if cb(&list[mi as usize]) {
            hi = mi;
        } else {
            lo = mi;
        }
    }
    hi
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bools_to_bytes() {
        let test_data = vec![
            (vec![true, true, true], vec![7]),
            (
                vec![false, false, false, false, false, false, false, false],
                vec![0b00000000],
            ),
            (
                vec![true, false, false, true, false, false, false, false],
                vec![0b10010000],
            ),
        ];
        for (data, result) in test_data {
            assert_eq!(bools_to_bytes(&data), result);
        }
    }

    #[test]
    fn test_common_prefix() {
        let test_data = vec![
            (vec![true, true, true], vec![true], vec![true]),
            (
                vec![false, false, true, false, false, false, false, false],
                vec![false, false, true, true],
                vec![false, false, true],
            ),
            (
                vec![true, false],
                vec![true, false, true],
                vec![true, false],
            ),
        ];
        for (data_left, data_right, result) in test_data {
            assert_eq!(common_prefix(&data_left, &data_right), result);
            assert_eq!(common_prefix(&data_right, &data_left), result);
        }
    }
}
