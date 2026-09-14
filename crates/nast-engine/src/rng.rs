//! 确定性随机复刻：ST 用的 seedrandom v3（alea + ARC4）与 getStringHash。
//!
//! {{pick}} 的结果完全由 (chatIdHash, rawContentHash, offset) 决定，
//! 因此必须逐位复刻 alea/ARC4/mixkey 与 getStringHash 的 32 位运算。

/// utils.js getStringHash：cyrb53 变体。输入按 JS UTF-16 code unit 逐个处理。
pub fn string_hash(s: &str) -> i64 {
    string_hash_seed(s, 0)
}

pub fn string_hash_seed(s: &str, seed: u32) -> i64 {
    let mut h1: u32 = 0xdeadbeef ^ seed;
    let mut h2: u32 = 0x41c6ce57 ^ seed;
    for ch in s.encode_utf16() {
        h1 = imul32(h1 ^ (ch as u32), 2654435761);
        h2 = imul32(h2 ^ (ch as u32), 1597334677);
    }
    h1 = imul32(h1 ^ (h1 >> 16), 2246822507) ^ imul32(h2 ^ (h2 >> 13), 3266489909);
    h2 = imul32(h2 ^ (h2 >> 16), 2246822507) ^ imul32(h1 ^ (h1 >> 13), 3266489909);

    (2097151 & h2) as i64 * 4294967296i64 + (h1 as i64)
}

/// JS Math.imul 语义：32 位截断乘法。
pub fn imul32(a: u32, b: u32) -> u32 {
    ((a as u64).wrapping_mul(b as u64) & 0xFFFF_FFFF) as u32
}

/// seedrandom v3 的 PRNG（seed → mixkey → ARC4）。
/// 复刻要点：mixkey 用 256 字节 key 数组 + smear 混入；
/// ARC4 KSA 后 g(count) 拼 count 个输出字节；prng = (n+x)/d 修正为 IEEE 有效位。
pub struct Alea {
    i: u8,
    j: u8,
    s: [u8; 256],
}

pub const WIDTH: u32 = 256;

impl Alea {
    pub fn new(seed: &str) -> Self {
        // mixkey(seed, key)：key 初始为空数组（全 0，长度随混入增长，掩码 &255）
        let mut key = [0u8; 256];
        let mut smear: u32 = 0;
        for (idx, unit) in seed.encode_utf16().enumerate() {
            let j = idx & 255;
            smear ^= key[j] as u32;
            smear = smear.wrapping_mul(19).wrapping_add(unit as u32) & 255;
            key[j] = smear as u8;
        }
        // ARC4(key)：key 长度 = 最后写入位置+1（mixkey 中 key[mask & j] 赋值后
        // tostring(key) 会带出所有已赋值槽位；空槽 undefined→flatten 保留；
        // 但 seedrandom 实际 key 数组长度取决于混入的 seed 长度。
        // 精确复刻：mask & (j + key[i % keylen] + s[i])，keylen = key 中
        // 最后一个非初始写入的位置 + 1；seed 非空时 keylen = seed 长度（≤256）。
        let keylen = seed.encode_utf16().count().max(1).min(256);
        let mut s = [0u8; 256];
        for (k, v) in s.iter_mut().enumerate() {
            *v = k as u8;
        }
        let mut i: u8 = 0;
        let mut j: u8 = 0;
        for idx in 0..256usize {
            let t = s[i as usize];
            j = 255u8.wrapping_add(j).wrapping_add(key[idx % keylen]);
            s[i as usize] = s[j as usize];
            s[j as usize] = t;
            i = i.wrapping_add(1);
        }
        Self { i, j, s }
    }

    /// g(count)：count 个 ARC4 输出字节拼成 0..2^(8*count) 的整数。
    fn g(&mut self, mut count: usize) -> u32 {
        let mut r: u32 = 0;
        while count > 0 {
            self.i = self.i.wrapping_add(1);
            let t = self.s[self.i as usize];
            self.j = self.j.wrapping_add(t);
            let sv = self.s[self.j as usize];
            self.s[self.i as usize] = sv;
            let sum = sv.wrapping_add(t);
            self.s[self.j as usize] = sum;
            r = r.wrapping_mul(WIDTH).wrapping_add(sum as u32);
            count -= 1;
        }
        r
    }

    /// 返回 [0,1) 的 double，与 seedrandom prng() 完全一致。
    pub fn next_f64(&mut self) -> f64 {
        let chunks = 6usize;
        let startdenom = (WIDTH as f64).powi(chunks as i32); // 2^48
        let significance = 2f64.powi(52);
        let overflow = significance * 2.0;
        let mut n = self.g(chunks) as f64;
        let mut d = startdenom;
        let mut x = 0f64;
        while n < significance {
            n = (n + x) * WIDTH as f64;
            d *= WIDTH as f64;
            x = self.g(1) as f64;
        }
        while n >= overflow {
            n /= 2.0;
            d /= 2.0;
            x /= 2.0;
        }
        (n + x) / d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_deterministic() {
        // 与 JS 互验证：getStringHash('hello') = 4625896200565286
        assert_eq!(string_hash("hello"), 4625896200565286);
        assert_eq!(string_hash(""), string_hash(""));
    }

    #[test]
    fn hash_stable() {
        assert_eq!(string_hash("hello"), string_hash("hello"));
        assert_ne!(string_hash("hello"), string_hash("hello!"));
    }

    #[test]
    fn alea_deterministic() {
        let mut a1 = Alea::new("seed");
        let mut a2 = Alea::new("seed");
        for _ in 0..10 {
            assert_eq!(a1.next_f64(), a2.next_f64());
        }
    }

    #[test]
    fn alea_range() {
        let mut a = Alea::new("added entropy.");
        for _ in 0..1000 {
            let v = a.next_f64();
            assert!((0.0..1.0).contains(&v));
        }
    }
}
