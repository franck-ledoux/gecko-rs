//! A picture as PNG bytes, and bytes as base64 text.
//!
//! Written here rather than taken from a crate because what is needed is
//! small and fixed: 8-bit RGB, one IDAT, deflate with the **fixed** Huffman
//! codes and a plain LZ77 match search. That compresses a rendering — large
//! runs of one background colour, rows that repeat the row above — to a
//! fraction of its size, which is all a picture for a reader to look at
//! asks for. The best compression a full encoder would find is not.
//!
//! The base64 is for carrying those bytes where only text goes: a read
//! answers with a string, and a PNG is not one.

/// An RGB picture, row by row from the top, three bytes a pixel.
pub fn encode(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    assert_eq!(
        rgb.len(),
        width as usize * height as usize * 3,
        "three bytes a pixel"
    );
    // Each row filtered by "Sub": a pixel less the one to its left, so a
    // run of one colour is a run of zeros, which is what LZ77 eats best.
    let stride = width as usize * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgb.chunks(stride) {
        raw.push(1);
        for (k, byte) in row.iter().enumerate() {
            let left = if k >= 3 { row[k - 3] } else { 0 };
            raw.push(byte.wrapping_sub(left));
        }
    }

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits, truecolour, deflate, adaptive filtering, not interlaced.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// A zlib stream: header, one fixed-Huffman deflate block, checksum.
pub(crate) fn zlib(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits::default();
    // CMF 0x78 (deflate, 32K window), FLG 0x01 makes the pair a multiple of 31.
    bits.out.extend_from_slice(&[0x78, 0x01]);
    bits.put(1, 1); // the last block
    bits.put(1, 2); // fixed codes
    deflate(data, &mut bits);
    bits.code(256); // end of block
    let mut out = bits.finish();
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

const WINDOW: usize = 32 * 1024;
const LONGEST: usize = 258;
/// How many earlier places a match is looked for at. Enough for the long
/// runs a rendering is made of; a picture is not worth more.
const TRIES: usize = 32;

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Greedy LZ77 over hash chains of three-byte prefixes.
fn deflate(data: &[u8], bits: &mut Bits) {
    const HASH: usize = 1 << 15;
    let mut head = vec![usize::MAX; HASH];
    let mut prev = vec![usize::MAX; data.len()];
    let hash = |i: usize| {
        let h = (u32::from(data[i]) << 10) ^ (u32::from(data[i + 1]) << 5) ^ u32::from(data[i + 2]);
        (h.wrapping_mul(2_654_435_761) >> 17) as usize % HASH
    };
    let insert = |i: usize, head: &mut Vec<usize>, prev: &mut Vec<usize>| {
        if i + 2 < data.len() {
            let h = hash(i);
            prev[i] = head[h];
            head[h] = i;
        }
    };

    let mut i = 0;
    while i < data.len() {
        let (mut best, mut at) = (0, 0);
        if i + 2 < data.len() {
            let mut candidate = head[hash(i)];
            let mut tries = TRIES;
            while candidate != usize::MAX && i - candidate <= WINDOW && tries > 0 {
                let most = LONGEST.min(data.len() - i);
                let mut n = 0;
                while n < most && data[candidate + n] == data[i + n] {
                    n += 1;
                }
                if n > best {
                    (best, at) = (n, candidate);
                    if n == most {
                        break;
                    }
                }
                candidate = prev[candidate];
                tries -= 1;
            }
        }
        if best >= 3 {
            bits.length(best);
            bits.distance(i - at);
            for k in i..i + best {
                insert(k, &mut head, &mut prev);
            }
            i += best;
        } else {
            bits.code(u16::from(data[i]));
            insert(i, &mut head, &mut prev);
            i += 1;
        }
    }
}

/// Bits, least significant first, as deflate packs them.
#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    word: u64,
    count: u32,
}

impl Bits {
    fn put(&mut self, value: u32, n: u32) {
        self.word |= u64::from(value) << self.count;
        self.count += n;
        while self.count >= 8 {
            self.out.push(self.word as u8);
            self.word >>= 8;
            self.count -= 8;
        }
    }

    /// A Huffman code, which deflate stores most significant bit first.
    fn huffman(&mut self, code: u32, n: u32) {
        self.put(code.reverse_bits() >> (32 - n), n);
    }

    /// A literal or length symbol, in the fixed code.
    fn code(&mut self, symbol: u16) {
        let s = u32::from(symbol);
        match s {
            0..=143 => self.huffman(0x30 + s, 8),
            144..=255 => self.huffman(0x190 + s - 144, 9),
            256..=279 => self.huffman(s - 256, 7),
            _ => self.huffman(0xc0 + s - 280, 8),
        }
    }

    fn length(&mut self, n: usize) {
        let k = LENGTH_BASE
            .iter()
            .rposition(|b| usize::from(*b) <= n)
            .expect("a length of 3 or more");
        self.code(257 + k as u16);
        self.put(
            (n - usize::from(LENGTH_BASE[k])) as u32,
            u32::from(LENGTH_EXTRA[k]),
        );
    }

    fn distance(&mut self, d: usize) {
        let k = DISTANCE_BASE
            .iter()
            .rposition(|b| usize::from(*b) <= d)
            .expect("a distance of 1 or more");
        self.huffman(k as u32, 5);
        self.put(
            (d - usize::from(DISTANCE_BASE[k])) as u32,
            u32::from(DISTANCE_EXTRA[k]),
        );
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.word as u8);
        }
        self.out
    }
}

/// Bytes as standard base64, with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Just enough of inflate to read back what `zlib` writes: one block,
    /// fixed codes. A round trip through it is the test that the bits are
    /// where deflate says they go.
    fn inflate(stream: &[u8]) -> Vec<u8> {
        assert_eq!(&stream[..2], &[0x78, 0x01]);
        let body = &stream[2..stream.len() - 4];
        let mut at = 0usize;
        let mut bit = |n: u32| -> u32 {
            let mut v = 0;
            for k in 0..n {
                let b = (body[at / 8] >> (at % 8)) & 1;
                v |= u32::from(b) << k;
                at += 1;
            }
            v
        };
        assert_eq!(bit(1), 1, "the last block");
        assert_eq!(bit(2), 1, "fixed codes");
        let mut out: Vec<u8> = Vec::new();
        loop {
            // Read a fixed-code symbol, most significant bit first.
            let mut code = 0u32;
            let mut len = 0;
            let symbol = loop {
                code = (code << 1) | bit(1);
                len += 1;
                match len {
                    7 if code <= 0x17 => break code + 256,
                    8 if (0x30..=0xbf).contains(&code) => break code - 0x30,
                    8 if (0xc0..=0xc7).contains(&code) => break code - 0xc0 + 280,
                    9 if (0x190..=0x1ff).contains(&code) => break code - 0x190 + 144,
                    _ if len > 9 => panic!("no such code"),
                    _ => {}
                }
            };
            match symbol {
                0..=255 => out.push(symbol as u8),
                256 => break,
                _ => {
                    let k = (symbol - 257) as usize;
                    let n = usize::from(LENGTH_BASE[k]) + bit(u32::from(LENGTH_EXTRA[k])) as usize;
                    let mut d = 0u32;
                    for _ in 0..5 {
                        d = (d << 1) | bit(1);
                    }
                    let d = d as usize;
                    let dist =
                        usize::from(DISTANCE_BASE[d]) + bit(u32::from(DISTANCE_EXTRA[d])) as usize;
                    for _ in 0..n {
                        out.push(out[out.len() - dist]);
                    }
                }
            }
        }
        assert_eq!(
            u32::from_be_bytes(stream[stream.len() - 4..].try_into().unwrap()),
            adler32(&out)
        );
        out
    }

    #[test]
    fn deflate_round_trips() {
        let mut data: Vec<u8> = Vec::new();
        data.extend(std::iter::repeat_n(7u8, 1000)); // a long run
        data.extend((0..3000u32).map(|k| (k * 7919 % 251) as u8)); // little to share
        data.extend_from_slice(b"abcabcabcabcabcabc the same again: abcabcabcabcabcabc");
        data.extend((0..70_000u32).map(|k| (k / 300) as u8)); // longer than the window
        let packed = zlib(&data);
        assert!(
            packed.len() < data.len() / 3,
            "{} bytes for {}",
            packed.len(),
            data.len()
        );
        assert_eq!(inflate(&packed), data);
        assert_eq!(inflate(&zlib(&[])), Vec::<u8>::new());
        assert_eq!(inflate(&zlib(b"ab")), b"ab");
    }

    #[test]
    fn a_png_is_signed_and_checksummed() {
        let rgb: Vec<u8> = (0..4 * 3 * 3).map(|k| k as u8).collect();
        let png = encode(4, 3, &rgb);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 4);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 3);
        // The header's checksum is over its kind and data.
        assert_eq!(
            u32::from_be_bytes(png[29..33].try_into().unwrap()),
            crc32(&png[12..29])
        );
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        // And what is inside is the rows, each filtered by Sub.
        let idat = 33 + 8;
        let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let rows = inflate(&png[idat..idat + len]);
        assert_eq!(rows.len(), 3 * (1 + 12));
        assert_eq!(rows[0], 1);
        assert_eq!(&rows[1..4], &[0, 1, 2], "the first pixel as it is");
        assert_eq!(&rows[4..7], &[3, 3, 3], "the next less the one to its left");
    }

    #[test]
    fn base64_pads_as_the_standard_says() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn crc_and_adler_agree_with_their_definitions() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }
}
