use alloc::format;
use alloc::vec::Vec;

use crate::{adler32, crc, deflate, zlib};

#[derive(Debug, Clone)]
pub(crate) struct IhdrChunk {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bit_depth: u8,
    pub(crate) color_type: u8,
    pub(crate) interlace_method: u8,
}

impl IhdrChunk {
    pub(crate) const COLOR_TYPE_GRAYSCALE: u8 = 0;
    pub(crate) const COLOR_TYPE_RGB: u8 = 2;
    pub(crate) const COLOR_TYPE_INDEXED: u8 = 3;
    pub(crate) const COLOR_TYPE_GRAYSCALE_ALPHA: u8 = 4;
    pub(crate) const COLOR_TYPE_RGBA: u8 = 6;
    const COMPRESSION_METHOD_DEFLATE: u8 = 0;
    const FILTER_METHOD_ADAPTIVE: u8 = 0;

    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        let mut data = [0u8; 13];
        data[0..4].copy_from_slice(&self.width.to_be_bytes());
        data[4..8].copy_from_slice(&self.height.to_be_bytes());
        data[8] = self.bit_depth;
        data[9] = self.color_type;
        data[10] = Self::COMPRESSION_METHOD_DEFLATE;
        data[11] = Self::FILTER_METHOD_ADAPTIVE;
        data[12] = self.interlace_method;
        append_chunk(out, b"IHDR", &data);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlteChunk<'a> {
    pub(crate) palette: &'a [[u8; 3]],
}

impl PlteChunk<'_> {
    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        let mut data = Vec::with_capacity(self.palette.len() * 3);
        for rgb in self.palette {
            data.extend_from_slice(rgb);
        }
        append_chunk(out, b"PLTE", &data);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TrnsChunk<'a> {
    pub(crate) data: &'a [u8],
}

impl TrnsChunk<'_> {
    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        append_chunk(out, b"tRNS", self.data);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IendChunk;

impl IendChunk {
    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        append_chunk(out, b"IEND", &[]);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IdatChunk<'a> {
    pub(crate) filtered_data: &'a [u8],
}

impl IdatChunk<'_> {
    pub(crate) fn append_to(&self, out: &mut Vec<u8>) -> crate::png_types::Result<()> {
        let chunk_data = zlib_wrap(self.filtered_data)?;
        append_chunk(out, b"IDAT", &chunk_data);
        Ok(())
    }
}

/// Wraps `filtered_data` in a self-contained zlib stream (header, deflate
/// payload, Adler-32 trailer).
///
/// This is the payload of an `IDAT` chunk, and it is also what `fdAT` chunks
/// of an APNG carry (modulo the leading sequence number, which the caller
/// contributes separately).
pub(crate) fn zlib_wrap(filtered_data: &[u8]) -> crate::png_types::Result<Vec<u8>> {
    let mut data = Vec::new();
    data.extend_from_slice(&zlib::ZLIB_HEADER);
    let deflated = deflate::compress(filtered_data).map_err(|error| {
        crate::png::Error::InvalidData(format!("invalid deflate stream: {error}").into())
    })?;
    data.extend_from_slice(&deflated);
    data.extend_from_slice(&adler32::calculate(filtered_data).to_be_bytes());
    Ok(data)
}

/// The `acTL` chunk of an APNG: animation control data that must appear before
/// the first `IDAT` chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcTlChunk {
    pub(crate) num_frames: u32,
    pub(crate) num_plays: u32,
}

impl AcTlChunk {
    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        let mut data = [0u8; 8];
        data[0..4].copy_from_slice(&self.num_frames.to_be_bytes());
        data[4..8].copy_from_slice(&self.num_plays.to_be_bytes());
        append_chunk(out, b"acTL", &data);
    }

    pub(crate) fn parse(data: &[u8]) -> crate::png_types::Result<Self> {
        let data = data
            .get(0..8)
            .ok_or_else(|| invalid_chunk_data("acTL", "expected 8 bytes"))?;
        Ok(Self {
            num_frames: read_u32(data, 0),
            num_plays: read_u32(data, 4),
        })
    }
}

/// The `fcTL` chunk of an APNG: per-frame control data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FcTlChunk {
    pub(crate) sequence_number: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) x_offset: u32,
    pub(crate) y_offset: u32,
    pub(crate) delay_num: u16,
    pub(crate) delay_den: u16,
    pub(crate) dispose_op: u8,
    pub(crate) blend_op: u8,
}

impl FcTlChunk {
    pub(crate) const DISPOSE_OP_NONE: u8 = 0;
    pub(crate) const DISPOSE_OP_BACKGROUND: u8 = 1;
    pub(crate) const DISPOSE_OP_PREVIOUS: u8 = 2;
    pub(crate) const BLEND_OP_SOURCE: u8 = 0;
    pub(crate) const BLEND_OP_OVER: u8 = 1;

    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        let mut data = [0u8; 26];
        data[0..4].copy_from_slice(&self.sequence_number.to_be_bytes());
        data[4..8].copy_from_slice(&self.width.to_be_bytes());
        data[8..12].copy_from_slice(&self.height.to_be_bytes());
        data[12..16].copy_from_slice(&self.x_offset.to_be_bytes());
        data[16..20].copy_from_slice(&self.y_offset.to_be_bytes());
        data[20..22].copy_from_slice(&self.delay_num.to_be_bytes());
        data[22..24].copy_from_slice(&self.delay_den.to_be_bytes());
        data[24] = self.dispose_op;
        data[25] = self.blend_op;
        append_chunk(out, b"fcTL", &data);
    }

    pub(crate) fn parse(data: &[u8]) -> crate::png_types::Result<Self> {
        let data = data
            .get(0..26)
            .ok_or_else(|| invalid_chunk_data("fcTL", "expected 26 bytes"))?;
        Ok(Self {
            sequence_number: read_u32(data, 0),
            width: read_u32(data, 4),
            height: read_u32(data, 8),
            x_offset: read_u32(data, 12),
            y_offset: read_u32(data, 16),
            delay_num: read_u16(data, 20),
            delay_den: read_u16(data, 22),
            dispose_op: data[24],
            blend_op: data[25],
        })
    }
}

/// An `fdAT` chunk: a sequence number followed by a fragment of the zlib stream
/// that encodes one animation frame. Multiple `fdAT` chunks may belong to a
/// single frame, in which case their fragments concatenate in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FdAtChunk<'a> {
    pub(crate) sequence_number: u32,
    pub(crate) data: &'a [u8],
}

impl<'a> FdAtChunk<'a> {
    pub(crate) fn append_to(&self, out: &mut Vec<u8>) {
        let mut data = Vec::with_capacity(4 + self.data.len());
        data.extend_from_slice(&self.sequence_number.to_be_bytes());
        data.extend_from_slice(self.data);
        append_chunk(out, b"fdAT", &data);
    }

    pub(crate) fn parse(data: &'a [u8]) -> crate::png_types::Result<Self> {
        let (sequence_number, data) = data
            .split_first_chunk::<4>()
            .ok_or_else(|| invalid_chunk_data("fdAT", "expected at least 4 bytes"))?;
        Ok(Self {
            sequence_number: u32::from_be_bytes(*sequence_number),
            data,
        })
    }
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(data[offset..offset + 2].try_into().expect("2 bytes"))
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().expect("4 bytes"))
}

fn invalid_chunk_data(chunk_type: &str, detail: &str) -> crate::png::Error {
    crate::png::Error::InvalidData(format!("invalid {chunk_type} chunk: {detail}").into())
}

fn append_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc::calculate(&[chunk_type.as_slice(), data]).to_be_bytes());
}
