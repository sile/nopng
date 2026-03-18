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

pub fn calculate(bytes: &[u8]) -> u32 {
    let mut crc = CRC_INITIAL;
    for &byte in bytes {
        crc = update_crc(crc, byte);
    }
    crc ^ CRC_INITIAL
}
