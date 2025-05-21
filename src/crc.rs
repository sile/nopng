use std::io::Write;

const CRC_INITIAL: u32 = 0xFFFFFFFF;

const CRC_TABLE: [u32; 256] = {
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
};

fn update_crc(crc: u32, byte: u8) -> u32 {
    (crc >> 8) ^ CRC_TABLE[((crc & 0xFF) ^ byte as u32) as usize]
}

// TODO: rename
#[derive(Debug)]
pub struct WriterWithCrc<W> {
    inner: W,
    crc: u32,
}

impl<W: Write> WriterWithCrc<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            crc: CRC_INITIAL,
        }
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.inner.write_all(&self.crc.to_be_bytes())
    }
}

impl<W: Write> Write for WriterWithCrc<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written_size = self.inner.write(buf)?;
        for b in &buf[..written_size] {
            self.crc = update_crc(self.crc, *b);
        }
        Ok(written_size)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
