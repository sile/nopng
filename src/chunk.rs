use std::io::Write;

use crate::crc::CrcWriter;

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
        writer.write_all(b"IDHR")?;
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

impl<'a> IdatChunk<'a> {
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
        Ok(())
    }
}

// fn create_idat_data(&self) -> std::io::Result<Vec<u8>> {
//     // use flate2::Compression;
//     // use flate2::write::ZlibEncoder;
//     // use std::io::Cursor;

//     // // Prepare scanlines with filtering
//     // let mut filtered_data = Vec::with_capacity(self.height * (1 + self.width * 4));

//     // // For each scanline, add filter type byte (0 = None) followed by raw pixel data
//     // for y in 0..self.height {
//     //     // Filter type 0 (None)
//     //     filtered_data.push(0);

//     //     // Add the raw pixel data for this scanline
//     //     let start = y * self.width * 4;
//     //     let end = start + self.width * 4;
//     //     filtered_data.extend_from_slice(&self.data[start..end]);
//     // }

//     // // Compress the filtered scanlines using zlib
//     // let mut compressed_data = Vec::new();
//     // {
//     //     let mut encoder =
//     //         ZlibEncoder::new(Cursor::new(&mut compressed_data), Compression::default());
//     //     encoder.write_all(&filtered_data)?;
//     //     encoder.finish()?;
//     // }

//     // Ok(compressed_data)
//     todo!()
// }
