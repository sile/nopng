const ADLER32_MOD: u32 = 65521; // Largest prime number less than 65536

pub fn calculate(data: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;

    for &byte in data {
        s1 = (s1 + byte as u32) % ADLER32_MOD;
        s2 = (s2 + s1) % ADLER32_MOD;
    }

    (s2 << 16) | s1
}
