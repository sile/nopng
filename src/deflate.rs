use std::io::Write;

#[derive(Debug)]
pub struct DeflateNoCompressionWriter<W> {
    inner: W,
    buffer: Vec<u8>,
    final_block: bool,
}

impl<W> DeflateNoCompressionWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            inner: writer,
            buffer: Vec::new(),
            final_block: false,
        }
    }

    // Sets whether this will be the final block in the DEFLATE stream
    pub fn set_final_block(&mut self, final_block: bool) {
        self.final_block = final_block;
    }
}

impl<W: Write> Write for DeflateNoCompressionWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Add data to our buffer
        self.buffer.extend_from_slice(buf);

        // Process complete blocks (max size 0xFFFF per DEFLATE spec)
        while self.buffer.len() >= 0xFFFF {
            self.write_block(0xFFFF)?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Write any remaining data in the buffer
        if !self.buffer.is_empty() {
            let len = self.buffer.len();
            self.write_block(len)?;
        }

        self.inner.flush()
    }
}

impl<W: Write> DeflateNoCompressionWriter<W> {
    // Writes a single non-compressed block of the specified size
    fn write_block(&mut self, size: usize) -> std::io::Result<()> {
        let is_final = self.final_block && size >= self.buffer.len();

        // Write block header: 1 bit for final block, 2 bits for block type (00 = no compression)
        let header_byte = if is_final { 0b0000_0001 } else { 0b0000_0000 };
        self.inner.write_all(&[header_byte])?;

        // Write LEN and NLEN (LEN's one's complement) - 16-bit values in little-endian
        let len = (size as u16).to_le_bytes();
        let nlen = (!size as u16).to_le_bytes();

        self.inner.write_all(&len)?;
        self.inner.write_all(&nlen)?;

        // Write the actual data
        self.inner.write_all(&self.buffer[..size])?;

        // Remove the written data from the buffer
        self.buffer.drain(0..size);

        Ok(())
    }
}
