use std::cmp;

use bitvec::prelude::*;

use crate::consts;
use crate::options;
use crate::types::{HashKind, HashWithKind};

pub fn get_iteration_mode<'a>(
    options: &options::IterationOption,
    opt: &'a mut Vec<u8>,
    has_prefix: bool,
) -> rocksdb::IteratorMode<'a> {
    let no_range = options.gte.is_none() && options.lte.is_none();
    if no_range {
        if options.reverse {
            rocksdb::IteratorMode::End
        } else {
            rocksdb::IteratorMode::Start
        }
    } else if options.reverse {
        let lte = options
            .lte
            .clone()
            .unwrap_or_else(|| vec![255; options.gte.as_ref().unwrap().len()]);
        *opt = if has_prefix {
            [consts::Prefix::STATE, lte.as_slice()].concat()
        } else {
            lte
        };
        rocksdb::IteratorMode::From(opt, rocksdb::Direction::Reverse)
    } else {
        let gte = options
            .gte
            .clone()
            .unwrap_or_else(|| vec![0; options.lte.as_ref().unwrap().len()]);
        *opt = if has_prefix {
            [consts::Prefix::STATE, gte.as_slice()].concat()
        } else {
            gte
        };
        rocksdb::IteratorMode::From(opt, rocksdb::Direction::Forward)
    }
}

pub fn is_key_out_of_range(
    options: &options::IterationOption,
    key: &[u8],
    counter: i64,
    has_prefix: bool,
) -> bool {
    if options.limit != -1 && counter >= options.limit {
        return true;
    }
    if options.reverse {
        if let Some(gte) = &options.gte {
            let cmp = if has_prefix {
                [consts::Prefix::STATE, gte].concat()
            } else {
                gte.to_vec()
            };
            if compare(key, &cmp) == cmp::Ordering::Less {
                return true;
            }
        }
    } else if let Some(lte) = &options.lte {
        let cmp = if has_prefix {
            [consts::Prefix::STATE, lte].concat()
        } else {
            lte.to_vec()
        };
        if compare(key, &cmp) == cmp::Ordering::Greater {
            return true;
        }
    }

    false
}

fn find_longer<'a>(a: &'a [bool], b: &'a [bool]) -> (&'a [bool], &'a [bool]) {
    match a.len().cmp(&b.len()) {
        cmp::Ordering::Greater => (a, b),
        cmp::Ordering::Less => (b, a),
        cmp::Ordering::Equal => (a, b),
    }
}

pub fn compare(a: &[u8], b: &[u8]) -> cmp::Ordering {
    for (ai, bi) in a.iter().zip(b.iter()) {
        match ai.cmp(bi) {
            cmp::Ordering::Equal => continue,
            ord => return ord,
        }
    }
    /* if every single element was equal, compare length */
    a.len().cmp(&b.len())
}

pub fn is_bit_set(bits: &[u8], i: usize) -> bool {
    bits[i / 8] << (i % 8) & 0x80 == 0x80
}

pub fn is_bytes_equal(a: &[u8], b: &[u8]) -> bool {
    compare(a, b) == cmp::Ordering::Equal
}

pub fn is_empty_hash(a: &[u8]) -> bool {
    compare(a, &vec![].hash_with_kind(HashKind::Empty)) == cmp::Ordering::Equal
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
    let mut target = vec![false; missing_byte];
    target.extend(a);

    for (i, v) in target.iter().enumerate() {
        if *v {
            result[i / 8] |= 0x80 >> (i % 8);
        }
    }
    result
}

pub fn bytes_to_bools(a: &[u8]) -> Vec<bool> {
    a.view_bits::<Msb0>()
        .iter()
        .by_vals()
        .collect::<Vec<bool>>()
}

pub fn common_prefix(a: &[bool], b: &[bool]) -> Vec<bool> {
    let mut result = vec![];
    let (longer, shorter) = find_longer(a, b);
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

pub fn bytes_in(list: &[Vec<u8>], a: &[u8]) -> bool {
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
