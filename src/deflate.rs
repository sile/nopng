use std::io::Write;

#[derive(Debug)]
pub struct DeflateNoCompressionEncoder;

impl DeflateNoCompressionEncoder {
    pub fn encode<W: Write>(&mut self, writer: &mut W, data: &[u8]) -> std::io::Result<()> {
        let mut remaining = data.len();
        let mut offset = 0;

        while remaining > 0 {
            // Determine block size (max 0xFFFF per DEFLATE spec)
            let size = std::cmp::min(remaining, 0xFFFF);

            // Check if this is the final block
            let is_final = size == remaining;

            // Write block header: 1 bit for final block, 2 bits for block type (00 = no compression)
            let header_byte = if is_final { 0b0000_0001 } else { 0b0000_0000 };
            writer.write_all(&[header_byte])?;

            // Write LEN and NLEN (LEN's one's complement) - 16-bit values in little-endian
            let len = (size as u16).to_le_bytes();
            let nlen = (!size as u16).to_le_bytes();

            writer.write_all(&len)?;
            writer.write_all(&nlen)?;

            // Write the actual data for this block
            writer.write_all(&data[offset..offset + size])?;

            // Update counters
            offset += size;
            remaining -= size;
        }

        writer.flush()
    }
}
