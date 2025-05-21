use std::io::Write;

use crate::chunk::{IendChunk, IhdrChunk};

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

        // Create and write IDAT chunk (with compressed image data)
        let idat_data = self.create_idat_data()?;
        let idat_chunk = self.create_chunk(*b"IDAT", idat_data);
        idat_chunk.write_to(writer)?;

        IendChunk.write_to(writer)?;

        Ok(())
    }

    fn create_idat_data(&self) -> std::io::Result<Vec<u8>> {
        // use flate2::Compression;
        // use flate2::write::ZlibEncoder;
        // use std::io::Cursor;

        // // Prepare scanlines with filtering
        // let mut filtered_data = Vec::with_capacity(self.height * (1 + self.width * 4));

        // // For each scanline, add filter type byte (0 = None) followed by raw pixel data
        // for y in 0..self.height {
        //     // Filter type 0 (None)
        //     filtered_data.push(0);

        //     // Add the raw pixel data for this scanline
        //     let start = y * self.width * 4;
        //     let end = start + self.width * 4;
        //     filtered_data.extend_from_slice(&self.data[start..end]);
        // }

        // // Compress the filtered scanlines using zlib
        // let mut compressed_data = Vec::new();
        // {
        //     let mut encoder =
        //         ZlibEncoder::new(Cursor::new(&mut compressed_data), Compression::default());
        //     encoder.write_all(&filtered_data)?;
        //     encoder.finish()?;
        // }

        // Ok(compressed_data)
        todo!()
    }

    fn create_chunk(&self, chunk_type: [u8; 4], data: Vec<u8>) -> PngChunk {
        let size = data.len() as u32;

        // Calculate CRC
        let mut crc = 0xFFFFFFFFu32;

        // Update CRC with chunk type
        for &byte in &chunk_type {
            crc = update_crc(crc, byte);
        }

        // Update CRC with chunk data
        for &byte in &data {
            crc = update_crc(crc, byte);
        }

        // Finalize CRC
        let crc = !crc;

        PngChunk {
            size,
            ty: chunk_type,
            data,
            crc,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PngChunk {
    pub size: u32,
    pub ty: [u8; 4],
    pub data: Vec<u8>,
    pub crc: u32,
}

impl PngChunk {
    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.size.to_be_bytes())?;
        writer.write_all(&self.ty)?;
        writer.write_all(&self.data)?;
        writer.write_all(&self.crc.to_be_bytes())?;
        Ok(())
    }

    pub fn check_crc(&self) -> bool {
        let calculated_crc = self.calculate_crc();
        calculated_crc == self.crc
    }

    fn calculate_crc(&self) -> u32 {
        // According to PNG spec, CRC is calculated over the chunk type and chunk data
        let mut crc = 0xFFFFFFFFu32;

        // Update CRC with chunk type
        for &byte in &self.ty {
            crc = update_crc(crc, byte);
        }

        // Update CRC with chunk data
        for &byte in &self.data {
            crc = update_crc(crc, byte);
        }

        // Finalize CRC
        !crc
    }
}

// CRC table for fast CRC calculation
const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;

    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;

        while k < 8 {
            if (c & 1) != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }

        table[n] = c;
        n += 1;
    }

    table
}

// CRC-32 table - precomputed for performance
const CRC_TABLE: [u32; 256] = make_crc_table();

// Function to update CRC
fn update_crc(crc: u32, byte: u8) -> u32 {
    (crc >> 8) ^ CRC_TABLE[((crc & 0xFF) ^ byte as u32) as usize]
}
