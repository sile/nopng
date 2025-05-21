use std::io::Write;

use crate::crc::WriterWithCrc;

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

        let mut writer = WriterWithCrc::new(writer);
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
