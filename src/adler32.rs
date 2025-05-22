use std::io::Write;

const ADLER32_INITIAL: u32 = 1;
const ADLER32_MOD: u32 = 65521; // Largest prime number less than 65536

#[derive(Debug)]
pub struct Adler32Writer<W> {
    inner: W,
    s1: u32,
    s2: u32,
}

impl<W: Write> Adler32Writer<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            s1: ADLER32_INITIAL & 0xFFFF,
            s2: (ADLER32_INITIAL >> 16) & 0xFFFF,
        }
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        let adler32 = (self.s2 << 16) | self.s1;
        self.inner.write_all(&adler32.to_be_bytes())
    }
}

impl<W: Write> Write for Adler32Writer<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written_size = self.inner.write(buf)?;
        for &byte in &buf[..written_size] {
            self.s1 = (self.s1 + byte as u32) % ADLER32_MOD;
            self.s2 = (self.s2 + self.s1) % ADLER32_MOD;
        }
        Ok(written_size)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
// TODO: Adler32Writer
