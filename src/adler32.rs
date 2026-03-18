const ADLER32_MOD: u32 = 65521; // Largest prime number less than 65536
const NMAX: usize = 5552; // Largest n such that 255*n*(n+1)/2 + (n+1)*(65520) <= 2^32 - 1

pub fn calculate(data: &[u8]) -> u32 {
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;

    for chunk in data.chunks(NMAX) {
        for &byte in chunk {
            s1 += byte as u32;
            s2 += s1;
        }
        s1 %= ADLER32_MOD;
        s2 %= ADLER32_MOD;
    }

    (s2 << 16) | s1
}
