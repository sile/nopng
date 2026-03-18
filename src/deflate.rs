use alloc::collections::BinaryHeap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp;
use core::error::Error as CoreError;

const MAX_BITS: usize = 15;
const END_OF_BLOCK: u16 = 256;
const WINDOW_SIZE: usize = 32_768;
const MAX_MATCH: usize = 258;
const MIN_MATCH: usize = 3;
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

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::UnexpectedEof => write!(f, "unexpected end of input"),
            Error::InvalidData(message) => f.write_str(message),
        }
    }
}

impl CoreError for Error {}

type Result<T> = core::result::Result<T, Error>;

pub fn compress(data: &[u8]) -> Result<Vec<u8>> {
    encode_dynamic_literals(data)
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
                all_code_lengths.extend(core::iter::repeat_n(last, repeat as usize));
            }
            17 => {
                let repeat = reader.read_bits(3)? + 3;
                all_code_lengths.extend(core::iter::repeat_n(0, repeat as usize));
            }
            18 => {
                let repeat = reader.read_bits(7)? + 11;
                all_code_lengths.extend(core::iter::repeat_n(0, repeat as usize));
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
    HuffmanDecoder::from_code_lengths(&[5u8; 30], Some(7), None)
}

fn encode_dynamic_literals(input: &[u8]) -> Result<Vec<u8>> {
    let symbols = lz77_symbols(input);
    let mut literal_frequencies = [0usize; 286];
    let mut distance_frequencies = [0usize; 30];
    let mut has_distance = false;
    for symbol in &symbols {
        literal_frequencies[symbol.code() as usize] += 1;
        if let Some((code, _, _)) = symbol.distance() {
            distance_frequencies[code as usize] += 1;
            has_distance = true;
        }
    }
    literal_frequencies[END_OF_BLOCK as usize] = 1;

    // Windows reportedly dislikes an empty distance table; emit a dummy symbol.
    if !has_distance {
        distance_frequencies[0] = 1;
    }

    let literal_lengths = length_limited_code_lengths(&literal_frequencies, MAX_BITS as u8);
    let distance_lengths = length_limited_code_lengths(&distance_frequencies, MAX_BITS as u8);
    let literal_encoder = HuffmanEncoder::from_code_lengths(&literal_lengths)?;
    let distance_encoder = HuffmanEncoder::from_code_lengths(&distance_lengths)?;

    let literal_code_count = cmp::max(
        257,
        literal_encoder.used_max_symbol().unwrap_or(0) as usize + 1,
    );
    let distance_code_count = cmp::max(
        1,
        distance_encoder.used_max_symbol().unwrap_or(0) as usize + 1,
    );

    let bitwidth_codes = build_bitwidth_codes(
        &literal_encoder,
        literal_code_count,
        &distance_encoder,
        distance_code_count,
    );
    let mut bitwidth_frequencies = [0usize; 19];
    for &(code, _, _) in &bitwidth_codes {
        bitwidth_frequencies[code as usize] += 1;
    }
    let bitwidth_lengths = length_limited_code_lengths(&bitwidth_frequencies, 7);
    let bitwidth_encoder = HuffmanEncoder::from_code_lengths(&bitwidth_lengths)?;
    let bitwidth_code_count = cmp::max(
        4,
        BITWIDTH_CODE_ORDER
            .iter()
            .rposition(|&index| bitwidth_encoder.code_width(index as u16) > 0)
            .map_or(0, |index| index + 1),
    );

    let mut writer = BitWriter::new();
    writer.write_bit(true);
    writer.write_bits(2, 0b10);
    writer.write_bits(5, (literal_code_count - 257) as u16);
    writer.write_bits(5, (distance_code_count - 1) as u16);
    writer.write_bits(4, (bitwidth_code_count - 4) as u16);
    for &index in BITWIDTH_CODE_ORDER.iter().take(bitwidth_code_count) {
        writer.write_bits(3, bitwidth_encoder.code_width(index as u16) as u16);
    }
    for &(code, extra_bits, extra) in &bitwidth_codes {
        bitwidth_encoder.encode(&mut writer, code as u16);
        if extra_bits > 0 {
            writer.write_bits(extra_bits, extra as u16);
        }
    }

    for symbol in &symbols {
        literal_encoder.encode(&mut writer, symbol.code());
        if let Some((bits, extra)) = symbol.extra_length() {
            writer.write_bits(bits, extra);
        }
        if let Some((code, bits, extra)) = symbol.distance() {
            distance_encoder.encode(&mut writer, code as u16);
            if bits > 0 {
                writer.write_bits(bits, extra);
            }
        }
    }
    literal_encoder.encode(&mut writer, END_OF_BLOCK);
    Ok(writer.finish())
}

fn lz77_symbols(input: &[u8]) -> Vec<DeflateSymbol> {
    let mut symbols = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some((distance, length)) = find_match(input, cursor) {
            symbols.push(DeflateSymbol::Pointer { length, distance });
            cursor += length;
        } else {
            symbols.push(DeflateSymbol::Literal(input[cursor]));
            cursor += 1;
        }
    }
    symbols
}

fn find_match(input: &[u8], cursor: usize) -> Option<(usize, usize)> {
    if cursor + MIN_MATCH > input.len() {
        return None;
    }

    let search_start = cursor.saturating_sub(WINDOW_SIZE);
    let max_length = (input.len() - cursor).min(MAX_MATCH);
    let mut best_distance = 0;
    let mut best_length = 0;

    for candidate in search_start..cursor {
        if input[candidate] != input[cursor] {
            continue;
        }
        let mut length = 1;
        while length < max_length && input[candidate + length] == input[cursor + length] {
            length += 1;
        }
        if length >= MIN_MATCH && length > best_length {
            best_length = length;
            best_distance = cursor - candidate;
            if length == max_length {
                break;
            }
        }
    }

    (best_length >= MIN_MATCH).then_some((best_distance, best_length))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HuffmanCode {
    width: u8,
    bits: u16,
}

impl HuffmanCode {
    const EMPTY: Self = Self { width: 0, bits: 0 };
}

#[derive(Debug, Clone)]
struct HuffmanEncoder {
    codes: Vec<HuffmanCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeflateSymbol {
    Literal(u8),
    Pointer { length: usize, distance: usize },
}

impl DeflateSymbol {
    fn code(self) -> u16 {
        match self {
            DeflateSymbol::Literal(byte) => u16::from(byte),
            DeflateSymbol::Pointer { length, .. } => length_to_symbol(length as u16).code,
        }
    }

    fn extra_length(self) -> Option<(u8, u16)> {
        match self {
            DeflateSymbol::Literal(_) => None,
            DeflateSymbol::Pointer { length, .. } => length_to_symbol(length as u16).extra,
        }
    }

    fn distance(self) -> Option<(u8, u8, u16)> {
        match self {
            DeflateSymbol::Literal(_) => None,
            DeflateSymbol::Pointer { distance, .. } => {
                let symbol = distance_to_symbol(distance as u16);
                Some((
                    symbol.code as u8,
                    symbol.extra.map_or(0, |(bits, _)| bits),
                    symbol.extra.map_or(0, |(_, extra)| extra),
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LengthSymbol {
    code: u16,
    extra: Option<(u8, u16)>,
}

fn length_to_symbol(length: u16) -> LengthSymbol {
    for (index, &(base, extra_bits)) in LENGTH_TABLE.iter().enumerate() {
        let span = if extra_bits == 0 {
            1
        } else {
            1u16 << extra_bits
        };
        let max = if index == LENGTH_TABLE.len() - 1 {
            base
        } else {
            base + span - 1
        };
        if (base..=max).contains(&length) {
            return LengthSymbol {
                code: 257 + index as u16,
                extra: (extra_bits > 0).then_some((extra_bits, length - base)),
            };
        }
    }
    unreachable!("invalid length: {length}");
}

fn distance_to_symbol(distance: u16) -> LengthSymbol {
    for (index, &(base, extra_bits)) in DISTANCE_TABLE.iter().enumerate() {
        let span = if extra_bits == 0 {
            1
        } else {
            1u16 << extra_bits
        };
        let max = base + span - 1;
        if (base..=max).contains(&distance) {
            return LengthSymbol {
                code: index as u16,
                extra: (extra_bits > 0).then_some((extra_bits, distance - base)),
            };
        }
    }
    unreachable!("invalid distance: {distance}");
}

impl HuffmanEncoder {
    fn from_code_lengths(lengths: &[u8]) -> Result<Self> {
        let mut codes = vec![HuffmanCode::EMPTY; lengths.len()];
        let mut symbols = lengths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 0)
            .map(|(symbol, width)| (symbol as u16, *width))
            .collect::<Vec<_>>();
        symbols.sort_by_key(|entry| entry.1);

        let mut code = 0u16;
        let mut previous_width = 0u8;
        for (symbol, width) in symbols {
            code <<= width - previous_width;
            codes[symbol as usize] = HuffmanCode {
                width,
                bits: reverse_bits(code, width),
            };
            code += 1;
            previous_width = width;
        }
        Ok(Self { codes })
    }

    fn encode(&self, writer: &mut BitWriter, symbol: u16) {
        let code = self.codes[symbol as usize];
        writer.write_bits(code.width, code.bits);
    }

    fn code_width(&self, symbol: u16) -> u8 {
        self.codes.get(symbol as usize).map_or(0, |code| code.width)
    }

    fn used_max_symbol(&self) -> Option<u16> {
        self.codes
            .iter()
            .rposition(|code| code.width > 0)
            .map(|index| index as u16)
    }
}

fn build_bitwidth_codes(
    literal: &HuffmanEncoder,
    literal_code_count: usize,
    distance: &HuffmanEncoder,
    distance_code_count: usize,
) -> Vec<(u8, u8, u8)> {
    #[derive(Debug)]
    struct RunLength {
        value: u8,
        count: usize,
    }

    let mut run_lengths = Vec::<RunLength>::new();
    for width in (0..literal_code_count)
        .map(|symbol| literal.code_width(symbol as u16))
        .chain((0..distance_code_count).map(|symbol| distance.code_width(symbol as u16)))
    {
        if run_lengths.last().is_some_and(|run| run.value == width) {
            run_lengths.last_mut().unwrap().count += 1;
        } else {
            run_lengths.push(RunLength {
                value: width,
                count: 1,
            });
        }
    }

    let mut codes = Vec::new();
    for run in run_lengths {
        if run.value == 0 {
            let mut count = run.count;
            while count >= 11 {
                let amount = cmp::min(138, count) as u8;
                codes.push((18, 7, amount - 11));
                count -= amount as usize;
            }
            if count >= 3 {
                codes.push((17, 3, count as u8 - 3));
                count = 0;
            }
            for _ in 0..count {
                codes.push((0, 0, 0));
            }
        } else {
            codes.push((run.value, 0, 0));
            let mut count = run.count - 1;
            while count >= 3 {
                let amount = cmp::min(6, count) as u8;
                codes.push((16, 2, amount - 3));
                count -= amount as usize;
            }
            for _ in 0..count {
                codes.push((run.value, 0, 0));
            }
        }
    }
    codes
}

fn length_limited_code_lengths(frequencies: &[usize], max_bitwidth: u8) -> Vec<u8> {
    let max_bitwidth = cmp::min(
        max_bitwidth,
        ordinary_huffman_optimal_max_bitwidth(frequencies),
    );
    package_merge_code_lengths(frequencies, max_bitwidth)
}

fn ordinary_huffman_optimal_max_bitwidth(frequencies: &[usize]) -> u8 {
    let mut heap = BinaryHeap::new();
    for &frequency in frequencies.iter().filter(|&&value| value > 0) {
        heap.push((-(frequency as isize), 0u8));
    }
    while heap.len() > 1 {
        let (weight1, width1) = heap.pop().unwrap();
        let (weight2, width2) = heap.pop().unwrap();
        heap.push((weight1 + weight2, 1 + cmp::max(width1, width2)));
    }
    cmp::max(1, heap.pop().map_or(0, |(_, width)| width))
}

fn package_merge_code_lengths(frequencies: &[usize], max_bitwidth: u8) -> Vec<u8> {
    #[derive(Debug, Clone)]
    struct Node {
        symbols: Vec<u16>,
        weight: usize,
    }

    impl Node {
        fn empty() -> Self {
            Self {
                symbols: Vec::new(),
                weight: 0,
            }
        }

        fn single(symbol: u16, weight: usize) -> Self {
            Self {
                symbols: vec![symbol],
                weight,
            }
        }

        fn merge(&mut self, other: Self) {
            self.weight += other.weight;
            self.symbols.extend(other.symbols);
        }
    }

    fn merge_nodes(left: Vec<Node>, right: Vec<Node>) -> Vec<Node> {
        let mut merged = Vec::with_capacity(left.len() + right.len());
        let mut left = left.into_iter().peekable();
        let mut right = right.into_iter().peekable();
        loop {
            match (
                left.peek().map(|node| node.weight),
                right.peek().map(|node| node.weight),
            ) {
                (None, None) => break,
                (Some(_), None) => {
                    merged.extend(left);
                    break;
                }
                (None, Some(_)) => {
                    merged.extend(right);
                    break;
                }
                (Some(left_weight), Some(right_weight)) => {
                    if left_weight < right_weight {
                        merged.push(left.next().unwrap());
                    } else {
                        merged.push(right.next().unwrap());
                    }
                }
            }
        }
        merged
    }

    fn package(mut nodes: Vec<Node>) -> Vec<Node> {
        if nodes.len() >= 2 {
            let new_len = nodes.len() / 2;
            for index in 0..new_len {
                nodes[index] = core::mem::replace(&mut nodes[index * 2], Node::empty());
                let other = core::mem::replace(&mut nodes[index * 2 + 1], Node::empty());
                nodes[index].merge(other);
            }
            nodes.truncate(new_len);
        }
        nodes
    }

    let mut source = frequencies
        .iter()
        .enumerate()
        .filter(|(_, frequency)| **frequency > 0)
        .map(|(symbol, frequency)| Node::single(symbol as u16, *frequency))
        .collect::<Vec<_>>();
    source.sort_by_key(|node| node.weight);

    let weighted = (0..max_bitwidth.saturating_sub(1)).fold(source.clone(), |weighted, _| {
        merge_nodes(package(weighted), source.clone())
    });

    let mut widths = vec![0u8; frequencies.len()];
    for symbol in package(weighted)
        .into_iter()
        .flat_map(|node| node.symbols.into_iter())
    {
        widths[symbol as usize] += 1;
    }
    widths
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

struct BitWriter {
    bytes: Vec<u8>,
    bit_buffer: u64,
    bit_count: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bit_buffer: 0,
            bit_count: 0,
        }
    }

    fn write_bit(&mut self, bit: bool) {
        self.write_bits(1, bit as u16);
    }

    fn write_bits(&mut self, bit_count: u8, bits: u16) {
        self.bit_buffer |= u64::from(bits) << self.bit_count;
        self.bit_count += bit_count;
        while self.bit_count >= 8 {
            self.bytes.push(self.bit_buffer as u8);
            self.bit_buffer >>= 8;
            self.bit_count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_count > 0 {
            self.bytes.push(self.bit_buffer as u8);
        }
        self.bytes
    }
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
    use alloc::vec;

    use super::{decompress, encode_dynamic_literals};

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

    #[test]
    fn encode_dynamic_literals_roundtrip() {
        let input = b"banana banana banana banana";
        let encoded = encode_dynamic_literals(input).unwrap();
        let decoded = decompress(&encoded).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn encode_dynamic_literals_uses_matches_for_repetition() {
        let input = vec![b'a'; 2048];
        let encoded = encode_dynamic_literals(&input).unwrap();
        let decoded = decompress(&encoded).unwrap();
        assert_eq!(decoded, input);
        assert!(encoded.len() < 64);
    }
}
