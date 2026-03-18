use alloc::format;
use alloc::vec::Vec;

use crate::{adler32, crc, deflate, zlib};

#[derive(Debug, Clone)]
pub struct IhdrChunk {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub interlace_method: u8,
}

impl IhdrChunk {
    pub const COLOR_TYPE_GRAYSCALE: u8 = 0;
    pub const COLOR_TYPE_RGB: u8 = 2;
    pub const COLOR_TYPE_INDEXED: u8 = 3;
    pub const COLOR_TYPE_GRAYSCALE_ALPHA: u8 = 4;
    pub const COLOR_TYPE_RGBA: u8 = 6;
    const COMPRESSION_METHOD_DEFLATE: u8 = 0;
    const FILTER_METHOD_ADAPTIVE: u8 = 0;

    pub fn append_to(&self, out: &mut Vec<u8>) {
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
pub struct PlteChunk<'a> {
    pub palette: &'a [[u8; 3]],
}

impl PlteChunk<'_> {
    pub fn append_to(&self, out: &mut Vec<u8>) {
        let mut data = Vec::with_capacity(self.palette.len() * 3);
        for rgb in self.palette {
            data.extend_from_slice(rgb);
        }
        append_chunk(out, b"PLTE", &data);
    }
}

#[derive(Debug, Clone)]
pub struct TrnsChunk<'a> {
    pub data: &'a [u8],
}

impl TrnsChunk<'_> {
    pub fn append_to(&self, out: &mut Vec<u8>) {
        append_chunk(out, b"tRNS", self.data);
    }
}

#[derive(Debug, Clone)]
pub struct IendChunk;

impl IendChunk {
    pub fn append_to(&self, out: &mut Vec<u8>) {
        append_chunk(out, b"IEND", &[]);
    }
}

#[derive(Debug, Clone)]
pub struct IdatChunk<'a> {
    pub filtered_data: &'a [u8],
}

impl IdatChunk<'_> {
    pub fn append_to(&self, out: &mut Vec<u8>) -> crate::png::Result<()> {
        let chunk_data = self.chunk_data()?;
        append_chunk(out, b"IDAT", &chunk_data);
        Ok(())
    }

    fn chunk_data(&self) -> crate::png::Result<Vec<u8>> {
        let mut data = Vec::new();
        data.extend_from_slice(&zlib::ZLIB_HEADER);
        let deflated = deflate::compress(self.filtered_data).map_err(|error| {
            crate::png::Error::InvalidData(format!("invalid deflate stream: {error}").into())
        })?;
        data.extend_from_slice(&deflated);
        data.extend_from_slice(&adler32::calculate(self.filtered_data).to_be_bytes());
        Ok(data)
    }
}

fn append_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc::calculate(&[chunk_type.as_slice(), data]).to_be_bytes());
}
