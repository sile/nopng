use std::io::Write;

use crate::{adler32, crc::CrcWriter, deflate::DeflateDynamicEncoder, zlib::ZlibHeader};

#[derive(Debug, Clone)]
pub struct IhdrChunk {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub interlace_method: u8,
}

impl IhdrChunk {
    const SIZE: u32 = 13;

    pub const COLOR_TYPE_GRAYSCALE: u8 = 0;
    pub const COLOR_TYPE_RGB: u8 = 2;
    pub const COLOR_TYPE_INDEXED: u8 = 3;
    pub const COLOR_TYPE_GRAYSCALE_ALPHA: u8 = 4;
    pub const COLOR_TYPE_RGBA: u8 = 6;
    const COMPRESSION_METHOD_DEFLATE: u8 = 0;
    const FILTER_METHOD_ADAPTIVE: u8 = 0;

    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&Self::SIZE.to_be_bytes())?;

        let mut writer = CrcWriter::new(writer);
        writer.write_all(b"IHDR")?;
        writer.write_all(&self.width.to_be_bytes())?;
        writer.write_all(&self.height.to_be_bytes())?;
        writer.write_all(&[self.bit_depth])?;
        writer.write_all(&[self.color_type])?;
        writer.write_all(&[Self::COMPRESSION_METHOD_DEFLATE])?;
        writer.write_all(&[Self::FILTER_METHOD_ADAPTIVE])?;
        writer.write_all(&[self.interlace_method])?;
        writer.finish()?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlteChunk<'a> {
    pub palette: &'a [[u8; 3]],
}

impl PlteChunk<'_> {
    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let size = (self.palette.len() * 3) as u32;
        writer.write_all(&size.to_be_bytes())?;

        let mut writer = CrcWriter::new(writer);
        writer.write_all(b"PLTE")?;
        for rgb in self.palette {
            writer.write_all(rgb)?;
        }
        writer.finish()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct TrnsChunk<'a> {
    pub data: &'a [u8],
}

impl TrnsChunk<'_> {
    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&(self.data.len() as u32).to_be_bytes())?;

        let mut writer = CrcWriter::new(writer);
        writer.write_all(b"tRNS")?;
        writer.write_all(self.data)?;
        writer.finish()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct IendChunk;

impl IendChunk {
    const SIZE: u32 = 0;

    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&Self::SIZE.to_be_bytes())?;

        let mut writer = CrcWriter::new(writer);
        writer.write_all(b"IEND")?;
        writer.finish()?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct IdatChunk<'a> {
    pub filtered_data: &'a [u8],
}

impl IdatChunk<'_> {
    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let mut chunk_data = Vec::new();
        self.write_chunk_data_to(&mut chunk_data)?;

        writer.write_all(&(chunk_data.len() as u32).to_be_bytes())?;

        let mut writer = CrcWriter::new(writer);
        writer.write_all(b"IDAT")?;
        writer.write_all(&chunk_data)?;
        writer.finish()?;

        Ok(())
    }

    fn write_chunk_data_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        ZlibHeader.write_to(writer)?;
        DeflateDynamicEncoder.encode(writer, self.filtered_data)?;
        writer.write_all(&adler32::calculate(self.filtered_data).to_be_bytes())?;

        Ok(())
    }
}
