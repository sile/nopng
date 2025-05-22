use std::io::Write;

use crate::chunk::{IdatChunk, IendChunk, IhdrChunk};

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone)]
pub struct PngRgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl PngRgbaImage {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        if (width * height * 4) as usize != data.len() {
            None
        } else {
            Some(Self {
                width,
                height,
                data,
            })
        }
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&PNG_SIGNATURE)?;

        IhdrChunk {
            width: self.width,
            height: self.height,
            bit_depth: 8,
            color_type: IhdrChunk::COLOR_TYPE_RGBA,
        }
        .write_to(writer)?;
        IdatChunk {
            stride: self.width as usize * 4,
            data: &self.data,
        }
        .write_to(writer)?;
        IendChunk.write_to(writer)?;

        Ok(())
    }
}
