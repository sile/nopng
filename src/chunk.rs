use std::io::Write;

use crate::{adler32, crc::CrcWriter, deflate::DeflateNoCompressionEncoder, zlib::ZlibHeader};

#[derive(Debug, Clone)]
pub struct IhdrChunk {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
}

impl IhdrChunk {
    const SIZE: u32 = 13;

    pub const COLOR_TYPE_RGBA: u8 = 6;
    const COMPRESSION_METHOD_DEFLATE: u8 = 0;
    const FILTER_METHOD_ADAPTIVE: u8 = 0;
    const INTERLACE_METHOD_NONE: u8 = 0;

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
        writer.write_all(&[Self::INTERLACE_METHOD_NONE])?;
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
    pub stride: usize,
    pub data: &'a [u8],
}

impl IdatChunk<'_> {
    const FILTER_TYPE_NONE: u8 = 0;

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
        let filtered_data = self
            .data
            .chunks(self.stride)
            .flat_map(|scanline| {
                std::iter::once(Self::FILTER_TYPE_NONE).chain(scanline.iter().copied())
            })
            .collect::<Vec<_>>();

        ZlibHeader.write_to(writer)?;
        DeflateNoCompressionEncoder.encode(writer, &filtered_data)?;
        writer.write_all(&adler32::calculate(&filtered_data).to_be_bytes())?;

        Ok(())
    }
}
