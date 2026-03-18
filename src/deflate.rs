use std::cmp;
use std::io::Write;

const MAX_BITS: usize = 15;
const END_OF_BLOCK: u16 = 256;
const BITWIDTH_CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
const LENGTH_TABLE: [(u16, u8); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];
const DISTANCE_TABLE: [(u16, u8); 30] = [
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 1),
    (7, 1),
    (9, 2),
    (13, 2),
    (17, 3),
    (25, 3),
    (33, 4),
    (49, 4),
    (65, 5),
    (97, 5),
    (129, 6),
    (193, 6),
    (257, 7),
    (385, 7),
    (513, 8),
    (769, 8),
    (1025, 9),
    (1537, 9),
    (2049, 10),
    (3073, 10),
    (4097, 11),
    (6145, 11),
    (8193, 12),
    (12_289, 12),
    (16_385, 13),
    (24_577, 13),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnexpectedEof,
    InvalidData(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UnexpectedEof => write!(f, "unexpected end of input"),
            Error::InvalidData(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct DeflateNoCompressionEncoder;

impl DeflateNoCompressionEncoder {
    pub fn encode<W: Write>(&mut self, writer: &mut W, data: &[u8]) -> std::io::Result<()> {
        let mut remaining = data.len();
        let mut offset = 0;

        while remaining > 0 {
            let size = std::cmp::min(remaining, 0xFFFF);
            let is_final = size == remaining;
            let header_byte = if is_final { 0b0000_0001 } else { 0b0000_0000 };
            writer.write_all(&[header_byte])?;

            let len = (size as u16).to_le_bytes();
            let nlen = (!size as u16).to_le_bytes();
            writer.write_all(&len)?;
            writer.write_all(&nlen)?;
            writer.write_all(&data[offset..offset + size])?;

            offset += size;
            remaining -= size;
        }

        writer.flush()
    }
}

pub fn decompress(input: &[u8]) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(input);
    let mut output = Vec::new();
    loop {
        let is_final = reader.read_bit()?;
        let block_type = reader.read_bits(2)?;
        match block_type {
            0b00 => decode_raw_block(&mut reader, &mut output)?,
            0b01 => decode_compressed_block(
                &mut reader,
                &fixed_literal_decoder()?,
                &fixed_distance_decoder()?,
                &mut output,
            )?,
            0b10 => {
                let (literal, distance) = read_dynamic_decoders(&mut reader)?;
                decode_compressed_block(&mut reader, &literal, &distance, &mut output)?;
            }
            0b11 => return Err(Error::InvalidData("reserved DEFLATE block type".into())),
            _ => unreachable!(),
        }
        if is_final {
            break;
        }
    }
    Ok(output)
}

fn decode_raw_block(reader: &mut BitReader<'_>, output: &mut Vec<u8>) -> Result<()> {
    reader.align_to_byte();
    let len = reader.read_u16_le()?;
    let nlen = reader.read_u16_le()?;
    if !len != nlen {
        return Err(Error::InvalidData(format!(
            "LEN={} is not the one's complement of NLEN={}",
            len, nlen
        )));
    }
    let bytes = reader.read_bytes(len as usize)?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn decode_compressed_block(
    reader: &mut BitReader<'_>,
    literal_decoder: &HuffmanDecoder,
    distance_decoder: &HuffmanDecoder,
    output: &mut Vec<u8>,
) -> Result<()> {
    loop {
        let symbol = literal_decoder.decode(reader)?;
        match symbol {
            0..=255 => output.push(symbol as u8),
            END_OF_BLOCK => return Ok(()),
            257..=285 => {
                let (base_length, extra_bits) = LENGTH_TABLE[(symbol - 257) as usize];
                let extra = if extra_bits == 0 {
                    0
                } else {
                    reader.read_bits(extra_bits)?
                };
                let length = base_length + extra;

                let distance_symbol = distance_decoder.decode(reader)?;
                let Some((base_distance, distance_extra_bits)) =
                    DISTANCE_TABLE.get(distance_symbol as usize).copied()
                else {
                    return Err(Error::InvalidData(format!(
                        "invalid distance symbol: {}",
                        distance_symbol
                    )));
                };
                let distance_extra = if distance_extra_bits == 0 {
                    0
                } else {
                    reader.read_bits(distance_extra_bits)?
                };
                let distance = (base_distance + distance_extra) as usize;
                copy_from_distance(output, distance, length as usize)?;
            }
            286 | 287 => {
                return Err(Error::InvalidData(format!(
                    "literal/length symbol {} must not appear in compressed data",
                    symbol
                )));
            }
            _ => unreachable!(),
        }
    }
}

fn copy_from_distance(output: &mut Vec<u8>, distance: usize, length: usize) -> Result<()> {
    if distance == 0 || distance > output.len() {
        return Err(Error::InvalidData(format!(
            "too long backward reference: output_len={}, distance={}",
            output.len(),
            distance
        )));
    }

    let start = output.len() - distance;
    for index in 0..length {
        let byte = output[start + (index % distance)];
        output.push(byte);
    }
    Ok(())
}

fn read_dynamic_decoders(reader: &mut BitReader<'_>) -> Result<(HuffmanDecoder, HuffmanDecoder)> {
    let literal_code_count = reader.read_bits(5)? + 257;
    let distance_code_count = reader.read_bits(5)? + 1;
    let bitwidth_code_count = reader.read_bits(4)? + 4;

    if distance_code_count as usize > DISTANCE_TABLE.len() {
        return Err(Error::InvalidData(format!(
            "HDIST is too large: {}",
            distance_code_count
        )));
    }

    let mut bitwidth_code_lengths = [0u8; 19];
    for &index in BITWIDTH_CODE_ORDER
        .iter()
        .take(bitwidth_code_count as usize)
    {
        bitwidth_code_lengths[index] = reader.read_bits(3)? as u8;
    }
    let bitwidth_decoder =
        HuffmanDecoder::from_code_lengths(&bitwidth_code_lengths, Some(1), None)?;

    let target_len = literal_code_count as usize + distance_code_count as usize;
    let mut all_code_lengths = Vec::with_capacity(target_len);
    while all_code_lengths.len() < target_len {
        let code = bitwidth_decoder.decode(reader)?;
        match code {
            0..=15 => all_code_lengths.push(code as u8),
            16 => {
                let repeat = reader.read_bits(2)? + 3;
                let Some(&last) = all_code_lengths.last() else {
                    return Err(Error::InvalidData(
                        "repeat code 16 without a previous code".into(),
                    ));
                };
                all_code_lengths.extend(std::iter::repeat_n(last, repeat as usize));
            }
            17 => {
                let repeat = reader.read_bits(3)? + 3;
                all_code_lengths.extend(std::iter::repeat_n(0, repeat as usize));
            }
            18 => {
                let repeat = reader.read_bits(7)? + 11;
                all_code_lengths.extend(std::iter::repeat_n(0, repeat as usize));
            }
            _ => unreachable!(),
        }
        if all_code_lengths.len() > target_len {
            return Err(Error::InvalidData(
                "dynamic huffman code lengths exceed the announced table size".into(),
            ));
        }
    }

    let literal_lengths = &all_code_lengths[..literal_code_count as usize];
    let distance_lengths = &all_code_lengths
        [literal_code_count as usize..literal_code_count as usize + distance_code_count as usize];
    let literal = HuffmanDecoder::from_code_lengths(literal_lengths, None, Some(END_OF_BLOCK))?;
    let distance =
        HuffmanDecoder::from_code_lengths(distance_lengths, Some(literal.safely_peek_bits), None)?;
    Ok((literal, distance))
}

fn fixed_literal_decoder() -> Result<HuffmanDecoder> {
    let mut lengths = vec![0u8; 288];
    for (index, length) in lengths.iter_mut().enumerate() {
        *length = match index {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            280..=287 => 8,
            _ => unreachable!(),
        };
    }
    HuffmanDecoder::from_code_lengths(&lengths, None, Some(END_OF_BLOCK))
}

fn fixed_distance_decoder() -> Result<HuffmanDecoder> {
    HuffmanDecoder::from_code_lengths(&vec![5u8; 30], Some(7), None)
}

fn reverse_bits(bits: u16, width: u8) -> u16 {
    let mut from = bits;
    let mut to = 0;
    for _ in 0..width {
        to <<= 1;
        to |= from & 1;
        from >>= 1;
    }
    to
}

struct HuffmanDecoder {
    table: Vec<u16>,
    safely_peek_bits: u8,
    max_bits: u8,
}

impl HuffmanDecoder {
    fn from_code_lengths(
        lengths: &[u8],
        safely_peek_bits: Option<u8>,
        eob_symbol: Option<u16>,
    ) -> Result<Self> {
        let max_bits = lengths.iter().copied().max().unwrap_or(0);
        if max_bits == 0 {
            return Err(Error::InvalidData("huffman table is empty".into()));
        }
        if max_bits as usize > MAX_BITS {
            return Err(Error::InvalidData(
                "huffman table uses too many bits".into(),
            ));
        }

        let table_len = 1usize << max_bits;
        let mut table = vec![u16::MAX; table_len];
        let mut entries = lengths
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, width)| *width > 0)
            .map(|(symbol, width)| (symbol as u16, width))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.1);

        let mut code = 0u16;
        let mut previous_width = 0u8;
        let mut actual_safely_peek = safely_peek_bits.unwrap_or(max_bits);

        for (symbol, width) in entries {
            code <<= width - previous_width;
            let reversed = reverse_bits(code, width);
            let value = (symbol << 5) | width as u16;
            let fill_count = 1usize << (max_bits - width);
            for padding in 0..fill_count {
                let index = ((padding as u16) << width | reversed) as usize;
                if table[index] != u16::MAX {
                    return Err(Error::InvalidData("conflicting huffman codes".into()));
                }
                table[index] = value;
            }
            if Some(symbol) == eob_symbol {
                actual_safely_peek = width;
            }
            code += 1;
            previous_width = width;
        }

        Ok(Self {
            table,
            safely_peek_bits: cmp::min(max_bits, actual_safely_peek.max(1)),
            max_bits,
        })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16> {
        let mut peek_bits = self.safely_peek_bits;
        loop {
            let bits = reader.peek_bits(peek_bits)?;
            let value = self.table[bits as usize];
            let width = (value & 0b1_1111) as u8;
            if width <= peek_bits && value != u16::MAX {
                reader.skip_bits(width)?;
                return Ok(value >> 5);
            }
            if width as usize > self.max_bits as usize || value == u16::MAX {
                return Err(Error::InvalidData("invalid huffman coded stream".into()));
            }
            peek_bits = width;
        }
    }
}

struct BitReader<'a> {
    input: &'a [u8],
    byte_index: usize,
    bit_buffer: u64,
    bit_count: u8,
}

impl<'a> BitReader<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            byte_index: 0,
            bit_buffer: 0,
            bit_count: 0,
        }
    }

    fn read_bit(&mut self) -> Result<bool> {
        Ok(self.read_bits(1)? != 0)
    }

    fn read_bits(&mut self, bit_count: u8) -> Result<u16> {
        let bits = self.peek_bits(bit_count)?;
        self.skip_bits(bit_count)?;
        Ok(bits)
    }

    fn peek_bits(&mut self, bit_count: u8) -> Result<u16> {
        while self.bit_count < bit_count {
            let Some(&next) = self.input.get(self.byte_index) else {
                return Err(Error::UnexpectedEof);
            };
            self.bit_buffer |= u64::from(next) << self.bit_count;
            self.bit_count += 8;
            self.byte_index += 1;
        }
        Ok((self.bit_buffer & ((1u64 << bit_count) - 1)) as u16)
    }

    fn skip_bits(&mut self, bit_count: u8) -> Result<()> {
        if self.bit_count < bit_count {
            self.peek_bits(bit_count)?;
        }
        self.bit_buffer >>= bit_count;
        self.bit_count -= bit_count;
        Ok(())
    }

    fn align_to_byte(&mut self) {
        self.bit_buffer = 0;
        self.bit_count = 0;
    }

    fn read_u16_le(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        self.align_to_byte();
        let end = self.byte_index + len;
        let Some(bytes) = self.input.get(self.byte_index..end) else {
            return Err(Error::UnexpectedEof);
        };
        self.byte_index = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::decompress;

    #[test]
    fn decode_known_fixed_block() {
        let input = [243, 72, 205, 201, 201, 87, 8, 207, 47, 202, 73, 81, 4, 0];
        let decoded = decompress(&input).unwrap();
        assert_eq!(decoded, b"Hello World!");
    }

    #[test]
    fn decode_known_raw_block() {
        let input = [
            1, 12, 0, 243, 255, 72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100, 33,
        ];
        let decoded = decompress(&input).unwrap();
        assert_eq!(decoded, b"Hello World!");
    }

    #[test]
    fn decode_known_dynamic_block() {
        let input = [75, 76, 42, 74, 76, 78, 76, 73, 4, 82, 10, 137, 216, 217, 0];
        let decoded = decompress(&input).unwrap();
        assert_eq!(decoded, b"abracadabra abracadabra abracadabra");
    }
}
