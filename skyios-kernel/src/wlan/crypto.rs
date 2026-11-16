use core::cmp::min;

fn rol32(v: u32, s: u32) -> u32 {
    (v << s) | (v >> (32 - s))
}

struct Sha1 {
    h: [u32; 5],
    n: u64,
    b: [u8; 64],
    bl: u32,
}

impl Sha1 {
    fn new() -> Self {
        Self {
            h: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            n: 0,
            b: [0; 64],
            bl: 0,
        }
    }

    fn block(&mut self, p: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = (p[4 * i] as u32) << 24
                | (p[4 * i + 1] as u32) << 16
                | (p[4 * i + 2] as u32) << 8
                | p[4 * i + 3] as u32;
        }
        for i in 16..80 {
            w[i] = rol32(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }
        let (mut a, mut b, mut d, mut e, mut f) =
            (self.h[0], self.h[1], self.h[2], self.h[3], self.h[4]);
        for i in 0..80 {
            let (x, k) = if i < 20 {
                ((b & d) | (!b & e), 0x5A827999u32)
            } else if i < 40 {
                (b ^ d ^ e, 0x6ED9EBA1)
            } else if i < 60 {
                ((b & d) | (b & e) | (d & e), 0x8F1BBCDC)
            } else {
                (b ^ d ^ e, 0xCA62C1D6)
            };
            let t = rol32(a, 5)
                .wrapping_add(x)
                .wrapping_add(f)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            f = e;
            e = d;
            d = rol32(b, 30);
            b = a;
            a = t;
        }
        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(d);
        self.h[3] = self.h[3].wrapping_add(e);
        self.h[4] = self.h[4].wrapping_add(f);
    }

    fn update(&mut self, data: &[u8]) {
        self.n += data.len() as u64;
        let mut off = 0usize;
        while off < data.len() {
            let take = min(64 - self.bl as usize, data.len() - off);
            let bl = self.bl as usize;
            self.b[bl..bl + take].copy_from_slice(&data[off..off + take]);
            self.bl += take as u32;
            off += take;
            if self.bl == 64 {
                let blk: [u8; 64] = self.b;
                self.block(&blk);
                self.bl = 0;
            }
        }
    }

    fn finish(mut self) -> [u8; 20] {
        let bits = self.n * 8;
        self.update(&[0x80]);
        while self.bl != 56 {
            self.update(&[0]);
        }
        let mut l = [0u8; 8];
        for i in 0..8 {
            l[i] = (bits >> (56 - 8 * i)) as u8;
        }
        self.update(&l);
        let mut out = [0u8; 20];
        for i in 0..5 {
            out[4 * i] = (self.h[i] >> 24) as u8;
            out[4 * i + 1] = (self.h[i] >> 16) as u8;
            out[4 * i + 2] = (self.h[i] >> 8) as u8;
            out[4 * i + 3] = self.h[i] as u8;
        }
        out
    }
}

pub fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let mut s = Sha1::new();
        s.update(key);
        let h = s.finish();
        k[..20].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = k[i] ^ 0x36;
        opad[i] = k[i] ^ 0x5c;
    }
    let mut s = Sha1::new();
    s.update(&ipad);
    for p in parts {
        s.update(p);
    }
    let inner = s.finish();
    let mut s = Sha1::new();
    s.update(&opad);
    s.update(&inner);
    s.finish()
}

pub fn pbkdf2_pmk(pass: &[u8], ssid: &[u8]) -> [u8; 32] {
    let mut pmk = [0u8; 32];
    for blk in 1u32..=2 {
        let cnt = blk.to_be_bytes();
        let mut u = hmac_sha1(pass, &[ssid, &cnt]);
        let mut t = u;
        for _ in 1..4096 {
            u = hmac_sha1(pass, &[&u]);
            for j in 0..20 {
                t[j] ^= u[j];
            }
        }
        let off = 20 * (blk as usize - 1);
        let take = if blk == 1 { 20 } else { 12 };
        pmk[off..off + take].copy_from_slice(&t[..take]);
    }
    pmk
}

pub fn prf(key: &[u8], label: &[u8], data: &[u8], out: &mut [u8]) {
    let mut pos = 0usize;
    let mut i = 0u8;
    while pos < out.len() {
        let mut buf = [0u8; 128];
        let ll = label.len();
        buf[..ll].copy_from_slice(label);
        buf[ll] = 0;
        buf[ll + 1..ll + 1 + data.len()].copy_from_slice(data);
        buf[ll + 1 + data.len()] = i;
        let h = hmac_sha1(key, &[&buf[..ll + 2 + data.len()]]);
        let take = min(out.len() - pos, 20);
        out[pos..pos + take].copy_from_slice(&h[..take]);
        pos += take;
        i += 1;
    }
}

static AES_SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab,
    0x76, 0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4,
    0x72, 0xc0, 0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71,
    0xd8, 0x31, 0x15, 0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2,
    0xeb, 0x27, 0xb2, 0x75, 0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6,
    0xb3, 0x29, 0xe3, 0x2f, 0x84, 0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb,
    0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf, 0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45,
    0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8, 0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5,
    0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2, 0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44,
    0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73, 0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a,
    0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb, 0xe0, 0x32, 0x3a, 0x0a, 0x49,
    0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79, 0xe7, 0xc8, 0x37, 0x6d,
    0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08, 0xba, 0x78, 0x25,
    0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a, 0x70, 0x3e,
    0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e, 0xe1,
    0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb,
    0x16,
];

fn aes_xt(x: u8) -> u8 {
    (x << 1) ^ if x & 0x80 != 0 { 0x1b } else { 0 }
}

fn aes_mul(mut a: u8, mut b: u8) -> u8 {
    let mut r = 0u8;
    while b != 0 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = aes_xt(a);
        b >>= 1;
    }
    r
}

fn aes128_expand(key: &[u8; 16]) -> [u8; 176] {
    let mut rk = [0u8; 176];
    rk[..16].copy_from_slice(key);
    let mut rcon = 1u8;
    let mut i = 16;
    while i < 176 {
        let mut t = [rk[i - 4], rk[i - 3], rk[i - 2], rk[i - 1]];
        if i % 16 == 0 {
            let x = t[0];
            t[0] = AES_SBOX[t[1] as usize] ^ rcon;
            t[1] = AES_SBOX[t[2] as usize];
            t[2] = AES_SBOX[t[3] as usize];
            t[3] = AES_SBOX[x as usize];
            rcon = aes_xt(rcon);
        }
        for j in 0..4 {
            rk[i + j] = rk[i - 16 + j] ^ t[j];
        }
        i += 4;
    }
    rk
}

fn aes128_decrypt(rk: &[u8; 176], s: &mut [u8; 16]) {
    let mut inv = [0u8; 256];
    for i in 0..256 {
        inv[AES_SBOX[i] as usize] = i as u8;
    }
    for j in 0..16 {
        s[j] ^= rk[160 + j];
    }
    for r in (0..=9).rev() {
        let mut t = [0u8; 16];
        for c in 0..4 {
            for row in 0..4 {
                t[4 * ((c + row) % 4) + row] = inv[s[4 * c + row] as usize];
            }
        }
        for j in 0..16 {
            t[j] ^= rk[16 * r + j];
        }
        if r == 0 {
            *s = t;
            break;
        }
        for c in 0..4 {
            let (a0, a1, a2, a3) = (t[4 * c], t[4 * c + 1], t[4 * c + 2], t[4 * c + 3]);
            s[4 * c] =
                aes_mul(a0, 14) ^ aes_mul(a1, 11) ^ aes_mul(a2, 13) ^ aes_mul(a3, 9);
            s[4 * c + 1] =
                aes_mul(a0, 9) ^ aes_mul(a1, 14) ^ aes_mul(a2, 11) ^ aes_mul(a3, 13);
            s[4 * c + 2] =
                aes_mul(a0, 13) ^ aes_mul(a1, 9) ^ aes_mul(a2, 14) ^ aes_mul(a3, 11);
            s[4 * c + 3] =
                aes_mul(a0, 11) ^ aes_mul(a1, 13) ^ aes_mul(a2, 9) ^ aes_mul(a3, 14);
        }
    }
}

pub fn aes_unwrap(kek: &[u8; 16], input: &[u8], out: &mut [u8]) -> bool {
    if input.len() < 24 || input.len() % 8 != 0 {
        return false;
    }
    let n = input.len() / 8 - 1;
    let rk = aes128_expand(kek);
    let mut a = [0u8; 8];
    a.copy_from_slice(&input[..8]);
    out[..input.len() - 8].copy_from_slice(&input[8..]);
    for j in (0..=5).rev() {
        for i in (1..=n).rev() {
            let t = (n as u64) * (j as u64) + (i as u64);
            let mut b = [0u8; 16];
            b[..8].copy_from_slice(&a);
            for k in 0..8 {
                b[7 - k] ^= (t >> (8 * k)) as u8;
            }
            b[8..16].copy_from_slice(&out[8 * (i - 1)..8 * i]);
            aes128_decrypt(&rk, &mut b);
            a.copy_from_slice(&b[..8]);
            out[8 * (i - 1)..8 * i].copy_from_slice(&b[8..16]);
        }
    }
    a == [0xA6; 8]
}

pub fn selftest() -> bool {
    let kek = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let ct: [u8; 24] = [
        0x1F, 0xA6, 0x8B, 0x0A, 0x81, 0x12, 0xB4, 0x47, 0xAE, 0xF3, 0x4B, 0xD8, 0xFB, 0x5A,
        0x7B, 0x82, 0x9D, 0x3E, 0x86, 0x23, 0x71, 0xD2, 0xCF, 0xE5,
    ];
    let pt: [u8; 16] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
        0xEE, 0xFF,
    ];
    let mut out = [0u8; 16];
    let a = aes_unwrap(&kek, &ct, &mut out) && out == pt;
    let pmk = pbkdf2_pmk(b"password", b"IEEE");
    let b = pmk[..4] == [0xf4, 0x2c, 0x6f, 0xc5];
    crate::log!(
        "SWEET WLAN: crypto self-test: key unwrap {}, pbkdf2 {}",
        if a { "ok" } else { "FAILED" },
        if b { "ok" } else { "FAILED" }
    );
    a && b
}
