use std::io::Write;

#[derive(Debug)]
pub struct ZlibHeader;

impl ZlibHeader {
    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Write the CMF and FLG bytes
        writer.write_all(&[
            // [CINFO=0111] 32k window size
            // [CM=1000] DEFLATE algorithm
            0b0111_1000,
            // [FLEVEL=10] Default compression level
            // [FDICT=0] no dictionary
            // [FCHECK=11100] check bits
            0b10_0_11100,
        ])
    }
}
