use std::io::Write;

#[derive(Debug)]
pub struct DeflateNoCompressionEncoder<W> {
    //
    inner: W,
    data: Vec<u8>,
}

impl<W: Write> DeflateNoCompressionEncoder<W> {
    pub fn new(writer: W, data: Vec<u8>) -> Self {
        Self {
            inner: writer,
            data,
        }
    }

    pub fn encode(&mut self) -> std::io::Result<()> {
        let mut remaining = self.data.len();
        let mut offset = 0;

        while remaining > 0 {
            // Determine block size (max 0xFFFF per DEFLATE spec)
            let size = std::cmp::min(remaining, 0xFFFF);

            // Check if this is the final block
            let is_final = size == remaining;

            // Write block header: 1 bit for final block, 2 bits for block type (00 = no compression)
            let header_byte = if is_final { 0b0000_0001 } else { 0b0000_0000 };
            self.inner.write_all(&[header_byte])?;

            // Write LEN and NLEN (LEN's one's complement) - 16-bit values in little-endian
            let len = (size as u16).to_le_bytes();
            let nlen = (!size as u16).to_le_bytes();

            self.inner.write_all(&len)?;
            self.inner.write_all(&nlen)?;

            // Write the actual data for this block
            self.inner.write_all(&self.data[offset..offset + size])?;

            // Update counters
            offset += size;
            remaining -= size;
        }

        self.inner.flush()
    }
}
