use std::io::Write;

pub const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone)]
pub struct PngFile {}

impl PngFile {}

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
