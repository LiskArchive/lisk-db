/// codec provides minimal lisk-codec feature to encode/decode diff for the state.
use thiserror::Error;

use crate::types::NestedVec;

const MAX_VARINT_LEN: usize = 10;

#[derive(Error, Debug)]
pub enum CodecError {
    #[error("Invalid bytes length")]
    InvalidBytesLength,
    #[error("Out of range")]
    OutOfRange,
    #[error("No termination code")]
    NoTermination,
    #[error("Invalid wire type")]
    InvalidWireType,
}

///Reader maintains the bytes and the state of read bytes during the decoding.
pub struct Reader<'a> {
    index: usize,
    end: usize,
    data: &'a [u8],
}

/// Writer maintains the bytes written during the encoding.
pub struct Writer {
    result: Vec<u8>,
    size: usize,
}

fn write_varint(value: u32) -> Vec<u8> {
    let mut value = value;
    let mut result = vec![0; MAX_VARINT_LEN];
    let mut index = 0;
    while value > 0x7f {
        result[index] = 0x80 | (value & 0x7f) as u8;
        value >>= 7;
        index += 1;
    }
    result[index] = value as u8;

    result[0..index + 1].to_vec()
}

/// read_varint from the given bytes starting from the offset.
/// it returns the value read as varint and the size it used.
fn read_varint(data: &[u8], offset: usize) -> Result<(u32, usize), CodecError> {
    let mut result: u32 = 0;
    let mut index = offset;
    let mut shift = 0;
    while shift < 32 {
        if index >= data.len() {
            return Err(CodecError::InvalidBytesLength);
        }
        let bit = data[index] as u32;
        index += 1;
        if index == offset + 5 && bit > 0x0f {
            return Err(CodecError::OutOfRange);
        }
        result |= (bit & 0x7f_u32) << shift;
        if (bit & 0x80) == 0 {
            return Ok((result, index - offset));
        }

        shift += 7;
    }
    Err(CodecError::NoTermination)
}

fn read_key(val: u32) -> Result<(u32, u32), CodecError> {
    let wire_type = val & 7;
    if wire_type != 0 && wire_type != 2 {
        return Err(CodecError::InvalidWireType);
    }
    let field_number = val >> 3;
    Ok((field_number, wire_type))
}

impl<'a> Reader<'a> {
    fn read_only_bytes(&mut self) -> Result<Vec<u8>, CodecError> {
        let (result, size) = read_varint(self.data, self.index)?;
        self.index += size;
        if result as usize > self.data.len() {
            return Err(CodecError::InvalidBytesLength);
        }
        let decoded = self.data[self.index..self.index + result as usize].to_vec();
        self.index += result as usize;

        Ok(decoded)
    }

    fn check(&mut self, field_number: u32) -> Result<bool, CodecError> {
        if self.index >= self.end {
            return Ok(false);
        }

        let (key, size) = read_varint(self.data, self.index)?;
        let (next_field_number, _) = read_key(key)?;
        if field_number != next_field_number {
            return Ok(false);
        }
        self.index += size;
        Ok(true)
    }

    /// new creates new reader from the given data.
    pub fn new(data: &'a [u8]) -> Self {
        let length = data.len();
        Self {
            data,
            index: 0,
            end: length,
        }
    }

    /// read_bytes_slice reads next field as slice of bytes slice.
    /// When next field does not match, it returns empty slice.
    pub fn read_bytes_slice(&mut self, field_number: u32) -> Result<NestedVec, CodecError> {
        let mut result = vec![];
        while self.index < self.end {
            let ok = self.check(field_number)?;
            if !ok {
                return Ok(result);
            }
            let value = self.read_only_bytes()?;
            result.push(value);
        }

        Ok(result)
    }

    /// read_bytes reads next field as bytes.
    /// When next field does not match, it returns empty bytes.
    pub fn read_bytes(&mut self, field_number: u32) -> Result<Vec<u8>, CodecError> {
        let ok = self.check(field_number)?;
        match ok {
            true => self.read_only_bytes(),
            false => Ok(vec![]),
        }
    }
}

impl Writer {
    fn write_key(&mut self, wire_type: u32, field_number: u32) {
        let key = (field_number << 3) | wire_type;
        let key_bytes = write_varint(key);
        self.size += key_bytes.len();
        self.result.extend(key_bytes);
    }

    fn write_varint(&mut self, val: u32) {
        let val_bytes = write_varint(val);
        self.size += val_bytes.len();
        self.result.extend(val_bytes);
    }

    pub fn new() -> Self {
        Self {
            result: vec![],
            size: 0,
        }
    }

    /// write_bytes encodes bytes slice to the writer with specified field number
    pub fn write_bytes(&mut self, field_number: u32, value: &[u8]) {
        self.write_key(2, field_number);
        self.write_varint(value.len() as u32);
        self.size += value.len();
        self.result.extend(value);
    }

    /// write_bytes encodes slice of bytes slice to the writer with specified field number
    pub fn write_bytes_slice(&mut self, field_number: u32, values: &[Vec<u8>]) {
        if values.is_empty() {
            return;
        }
        for val in values.iter() {
            self.write_bytes(field_number, val);
        }
    }

    pub fn result(&self) -> &Vec<u8> {
        &self.result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_reader() {
        let reader = Reader::new(&[1, 2, 3, 4, 5]);
        assert_eq!(reader.data, &[1, 2, 3, 4, 5]);
        assert_eq!(reader.end, 5);
        assert_eq!(reader.index, 0);
    }

    #[test]
    fn test_reader_read_bytes_slice() {
        let mut writer = Writer::new();
        writer.write_bytes(1, &[1, 2, 3, 4, 5]);
        writer.write_bytes(2, &[6, 7, 8, 9, 10]);
        writer.write_bytes(3, &[11, 12, 13, 14, 15]);

        let mut reader = Reader::new(writer.result());

        let res = reader.read_bytes_slice(1).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], [1, 2, 3, 4, 5].to_vec());

        let res = reader.read_bytes_slice(2).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], [6, 7, 8, 9, 10].to_vec());

        let res = reader.read_bytes_slice(3).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], [11, 12, 13, 14, 15].to_vec());
    }

    #[test]
    fn test_reader_read_bytes() {
        let mut writer = Writer::new();
        writer.write_bytes(1, &[1, 2, 3, 4, 5]);
        writer.write_bytes(1, &[6, 7, 8, 9, 10]);
        writer.write_bytes(2, &[11, 12, 13, 14, 15]);

        let mut reader = Reader::new(writer.result());

        let res = reader.read_bytes(1).unwrap();
        assert_eq!(res, [1, 2, 3, 4, 5].to_vec());

        let res = reader.read_bytes(1).unwrap();
        assert_eq!(res, [6, 7, 8, 9, 10].to_vec());

        let res = reader.read_bytes(1).unwrap();
        assert_eq!(res, vec![]);

        let res = reader.read_bytes(2).unwrap();
        assert_eq!(res, [11, 12, 13, 14, 15].to_vec());

        let res = reader.read_bytes(2).unwrap();
        assert_eq!(res, vec![]);
    }

    #[test]
    fn test_new_writer() {
        let writer = Writer::new();
        assert_eq!(writer.result, vec![]);
        assert_eq!(writer.size, 0);
    }

    #[test]
    fn test_writer_write_bytes_slice() {
        let mut writer = Writer::new();
        writer.write_bytes_slice(1, &[vec![1, 2, 3, 4, 5], vec![6, 7, 8, 9, 10]]);
        writer.write_bytes_slice(2, &[vec![11, 12, 13, 14, 15], vec![16, 17, 18, 19, 20]]);

        let mut reader = Reader::new(writer.result());

        let res = reader.read_bytes(1).unwrap();
        assert_eq!(res, [1, 2, 3, 4, 5].to_vec());

        let res = reader.read_bytes(1).unwrap();
        assert_eq!(res, [6, 7, 8, 9, 10].to_vec());

        let res = reader.read_bytes(1).unwrap();
        assert_eq!(res, vec![]);

        let res = reader.read_bytes(2).unwrap();
        assert_eq!(res, [11, 12, 13, 14, 15].to_vec());

        let res = reader.read_bytes(2).unwrap();
        assert_eq!(res, [16, 17, 18, 19, 20].to_vec());

        let res = reader.read_bytes(2).unwrap();
        assert_eq!(res, vec![]);
    }
}
