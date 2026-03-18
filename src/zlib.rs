#[expect(clippy::unusual_byte_groupings)]
pub const ZLIB_HEADER: [u8; 2] = [
    // [CINFO=0111] 32k window size
    // [CM=1000] DEFLATE algorithm
    0b0111_1000,
    // [FLEVEL=10] Default compression level
    // [FDICT=0] no dictionary
    // [FCHECK=11100] check bits
    0b10_0_11100,
];
