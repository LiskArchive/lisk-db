use std::cmp;

use bitvec::prelude::*;

use crate::sparse_merkle_tree::smt::EMPTY_HASH;

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
    compare(a, &EMPTY_HASH) == cmp::Ordering::Equal
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
        } else {
            return result;
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

pub fn array_equal_bool(a: &[bool], b: &[bool]) -> bool {
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

pub fn binary_search<T>(list: &[T], callback: impl Fn(&T) -> bool) -> i32 {
    let mut lo = -1;
    let mut hi = list.len() as i32;
    while 1 + lo < hi {
        let mi = lo + ((hi - lo) >> 1);
        if callback(&list[mi as usize]) {
            hi = mi;
        } else {
            lo = mi;
        }
    }
    hi
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    #[test]
    fn test_compare() {
        assert_eq!(Ordering::Equal, compare(&[1, 2, 3], &[1, 2, 3]));
        assert_eq!(Ordering::Less, compare(&[1, 2, 3], &[3, 2, 1]));
        assert_eq!(Ordering::Greater, compare(&[3, 2, 1], &[1, 2, 3]));
    }

    #[test]
    fn test_is_bit_set() {
        assert!(is_bit_set(&[0b10000000], 0));
        assert!(is_bit_set(&[0b01000000], 1));
        assert!(is_bit_set(&[0b00100000], 2));
        assert!(is_bit_set(&[0b00010000], 3));
        assert!(is_bit_set(&[0b00001000], 4));
        assert!(is_bit_set(&[0b00000100], 5));
        assert!(is_bit_set(&[0b00000010], 6));
        assert!(is_bit_set(&[0b00000001], 7));

        assert!(is_bit_set(&[0b00000000, 0b10000000], 8));
        assert!(is_bit_set(&[0b00000000, 0b01000000], 9));
        assert!(is_bit_set(&[0b00000000, 0b00100000], 10));
        assert!(is_bit_set(&[0b00000000, 0b00010000], 11));
        assert!(is_bit_set(&[0b00000000, 0b00001000], 12));
        assert!(is_bit_set(&[0b00000000, 0b00000100], 13));
        assert!(is_bit_set(&[0b00000000, 0b00000010], 14));
        assert!(is_bit_set(&[0b00000000, 0b00000001], 15));

        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b10000000], 16));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b01000000], 17));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b00100000], 18));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b00010000], 19));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b00001000], 20));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b00000100], 21));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b00000010], 22));
        assert!(is_bit_set(&[0b00000000, 0b00000000, 0b00000001], 23));

        assert!(!is_bit_set(&[0b10000000], 2));
        assert!(!is_bit_set(&[0b10000000], 3));
        assert!(!is_bit_set(&[0b00000100], 2));

        assert!(!is_bit_set(&[0b00000000, 0b00100000], 11));
        assert!(!is_bit_set(&[0b01110000, 0b00000100], 10));
        assert!(!is_bit_set(&[0b00001110, 0b00010010], 10));

        assert!(!is_bit_set(&[0b01110000, 0b00001100, 0b01000000], 16));
        assert!(!is_bit_set(&[0b00000000, 0b00111000, 0b00000011], 17));
        assert!(!is_bit_set(&[0b00111100, 0b01100010, 0b11011110], 18));
    }

    #[test]
    fn test_is_bytes_equal() {
        assert!(is_bytes_equal(&[1, 2, 3], &[1, 2, 3]));
        assert!(is_bytes_equal(&[3, 2, 1], &[3, 2, 1]));
        assert!(!is_bytes_equal(&[1, 2, 2], &[1, 2, 3]));
        assert!(!is_bytes_equal(&[2, 2, 3], &[1, 2, 3]));
    }

    #[test]
    fn test_is_empty_hash() {
        assert!(is_empty_hash(&EMPTY_HASH));
        assert!(!is_empty_hash(&[
            227, 176, 196, 66, 152, 252, 27, 20, 154, 251, 244, 200, 153, 111, 185, 36, 39, 174,
            65, 228, 100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184, 85,
        ]));
    }

    #[test]
    fn test_is_bools_equal() {
        assert!(is_bools_equal(&[true, false, true], &[true, false, true]));
        assert!(is_bools_equal(
            &[false, false, false],
            &[false, false, false]
        ));
        assert!(is_bools_equal(&[true, true, true], &[true, true, true]));
        assert!(!is_bools_equal(&[true, false, true], &[true, true, true]));
        assert!(!is_bools_equal(&[true, false, true], &[true, false, false]));
        assert!(!is_bools_equal(&[true, false, true], &[false, false, true]));
    }

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
    fn test_bytes_to_bools() {
        assert_eq!(
            bytes_to_bools(&[0b00000000]),
            vec![false, false, false, false, false, false, false, false]
        );
        assert_eq!(
            bytes_to_bools(&[0b10010000]),
            vec![true, false, false, true, false, false, false, false]
        );
        assert_eq!(
            bytes_to_bools(&[0b10010000, 0b00000000]),
            vec![
                true, false, false, true, false, false, false, false, false, false, false, false,
                false, false, false, false
            ]
        );
        assert_eq!(bytes_to_bools(&[0b00000000, 0b00000000]), vec![false; 16]);
        assert_eq!(bytes_to_bools(&[0b11111111, 0b11111111]), vec![true; 16]);
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
            (
                vec![true, false, true, true],
                vec![true, true, true, true],
                vec![true],
            ),
            (
                vec![true, false, false, true, false],
                vec![true, false, true, true, false],
                vec![true, false],
            ),
        ];
        for (data_left, data_right, result) in test_data {
            assert_eq!(common_prefix(&data_left, &data_right), result);
            assert_eq!(common_prefix(&data_right, &data_left), result);
        }
    }

    #[test]
    fn test_strip_left_false() {
        let test_data = vec![
            (vec![false, false, false], vec![]),
            (vec![false, false, true], vec![true]),
            (vec![false, true, false], vec![true, false]),
            (vec![true, false, false], vec![true, false, false]),
            (
                vec![
                    false, false, false, false, true, false, false, true, false, false, true, true,
                ],
                vec![true, false, false, true, false, false, true, true],
            ),
        ];
        for (data, result) in test_data {
            assert_eq!(strip_left_false(&data), result);
        }
    }

    #[test]
    fn test_bytes_in() {
        assert!(bytes_in(&[vec![0b00000000]], &[0b00000000]));
        assert!(bytes_in(&[vec![0b00000001]], &[0b00000001]));
        assert!(bytes_in(
            &[vec![0b00000001], vec![0b00000001]],
            &[0b00000001]
        ));
        assert!(bytes_in(&[vec![0b00000001]], &[0b00000001]));
        assert!(bytes_in(
            &[vec![0b11010001], vec![0b11010001]],
            &[0b11010001]
        ));
        assert!(bytes_in(
            &[vec![0b11010001], vec![0b11010001], vec![0b11010001]],
            &[0b11010001]
        ));

        assert!(!bytes_in(&[vec![0b00000000]], &[0b01000000]));
        assert!(!bytes_in(
            &[vec![0b01110000], vec![0b01000000]],
            &[0b00000001]
        ));
        assert!(!bytes_in(
            &[vec![0b01100001], vec![0b00100001]],
            &[0b00000001]
        ));
        assert!(!bytes_in(
            &[vec![0b11010011], vec![0b11000001], vec![0b11011001]],
            &[0b11010001]
        ));
    }

    #[test]
    fn test_array_equal_bool() {
        assert!(array_equal_bool(&[true, false, true], &[true, false, true]));
        assert!(array_equal_bool(
            &[false, false, false],
            &[false, false, false]
        ));
        assert!(array_equal_bool(&[true, true, true], &[true, true, true]));

        assert!(!array_equal_bool(&[true, false, true], &[true, true, true]));
        assert!(!array_equal_bool(
            &[true, false, true],
            &[true, false, false]
        ));
        assert!(!array_equal_bool(
            &[true, false, true],
            &[false, false, true]
        ));

        assert!(!array_equal_bool(&[true, true, true], &[true, true]));
        assert!(!array_equal_bool(&[true, true], &[true, true, true]));
        assert!(!array_equal_bool(&[false, false, false], &[false, false]));
        assert!(!array_equal_bool(&[false, false], &[false, false, false]));
    }

    #[test]
    fn test_binary_search() {
        let test_data = vec![
            (vec![vec![0b00000000]], vec![0b00000000], 1),
            (vec![vec![0b00000001]], vec![0b00000001], 1),
            (
                vec![vec![0b00000001], vec![0b00000001]],
                vec![0b00000001],
                2,
            ),
            (
                vec![vec![0b00000001], vec![0b00000001]],
                vec![0b00000001],
                2,
            ),
            (
                vec![vec![0b11011001], vec![0b11010001]],
                vec![0b11010001],
                0,
            ),
            (
                vec![vec![0b11011001], vec![0b11010001], vec![0b11010011]],
                vec![0b11010001],
                2,
            ),
            (
                vec![
                    vec![0b11010001],
                    vec![0b11011001],
                    vec![0b11110001],
                    vec![0b11010011],
                ],
                vec![0b11010001],
                1,
            ),
            (
                vec![
                    vec![0b00010001],
                    vec![0b01011001],
                    vec![0b11110001],
                    vec![0b11010001],
                    vec![0b01010011],
                    vec![0b01010000],
                ],
                vec![0b01010000],
                1,
            ),
        ];

        for (data, key, result) in test_data {
            assert_eq!(
                binary_search(&data, |val| { compare(&key, val) == cmp::Ordering::Less }),
                result
            );
        }
    }
}
