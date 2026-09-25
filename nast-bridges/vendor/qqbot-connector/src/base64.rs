//! 标准字母表 Base64 编解码（RFC 4648），替代独立的 base64 crate。
//!
//! 仅覆盖本 crate 需要的能力：STD 编码（带 `=` 填充）与解码（要求输入合法，
//! 允许省略尾部填充，与常见宽松实现一致）。

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PAD: u8 = b'=';

/// 将任意字节序列编码为标准 Base64 字符串（带 `=` 填充）。
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        } else {
            out.push(PAD as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        } else {
            out.push(PAD as char);
        }
    }
    out
}

fn decode_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// 解码标准 Base64 字符串。
///
/// 接受带或不带尾部 `=` 填充的输入；其余任何非法字符、非法长度或位置
/// 错误的填充都会返回错误。
pub fn decode(input: &str) -> Result<Vec<u8>, String> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    // 拆出有效负载与填充部分。
    let mut payload_end = bytes.len();
    let mut padded_len = 0usize;
    while payload_end > 0 && bytes[payload_end - 1] == PAD {
        payload_end -= 1;
        padded_len += 1;
    }
    if padded_len > 2 {
        return Err("too much padding".into());
    }
    let payload = &bytes[..payload_end];

    // 带填充时总长度必须是 4 的倍数；不带填充时仅禁止 mod 4 == 1 的长度。
    if padded_len > 0 {
        if (payload_end + padded_len) % 4 != 0 {
            return Err("invalid length".into());
        }
    } else if payload.len() % 4 == 1 {
        return Err("invalid length".into());
    }

    let mut out = Vec::with_capacity(payload_end / 4 * 3 + 2);
    for (i, group) in payload.chunks(4).enumerate() {
        let is_last = i == payload.len().div_ceil(4) - 1;
        if !is_last && group.len() != 4 {
            return Err("invalid length".into());
        }
        let mut acc: u32 = 0;
        for &b in group {
            let v = decode_value(b).ok_or_else(|| format!("invalid byte {b:#04x}"))?;
            acc = (acc << 6) | v as u32;
        }
        match group.len() {
            4 => {
                out.push((acc >> 16) as u8);
                out.push((acc >> 8) as u8);
                out.push(acc as u8);
            }
            // 尾组长 3：允许无填充或 1 个填充字节。
            3 => {
                if padded_len > 1 {
                    return Err("incorrect padding".into());
                }
                out.push((acc >> 10) as u8);
                out.push((acc >> 2) as u8);
            }
            // 尾组长 2：允许无填充或 2 个填充字节。
            2 => {
                if padded_len != 0 && padded_len != 2 {
                    return Err("incorrect padding".into());
                }
                out.push((acc >> 4) as u8);
            }
            _ => return Err("invalid length".into()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_vectors() {
        // RFC 4648 §10 测试向量。
        for (raw, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(raw.as_bytes()), encoded, "encode {raw:?}");
            assert_eq!(decode(encoded).unwrap(), raw.as_bytes(), "decode {encoded}");
        }
    }

    #[test]
    fn roundtrip_binary() {
        // 覆盖所有余数分支 + 二进制内容（含 0x00/0xff）。
        for len in 0..64usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 % 256) as u8).collect();
            let encoded = encode(&data);
            assert_eq!(decode(&encoded).unwrap(), data, "len={len}");
        }
    }

    #[test]
    fn decode_accepts_missing_padding() {
        // 无填充输入与补齐填充后解码结果一致（尾部不满一个字节的位被丢弃）。
        assert_eq!(decode("Zg").unwrap(), b"f");
        assert_eq!(decode("Zm8").unwrap(), b"fo");
        assert_eq!(decode("Zm9v").unwrap(), b"foo");
        assert_eq!(decode("Zm9vYg").unwrap(), b"foob");
    }

    #[test]
    fn decode_rejects_invalid() {
        // 长度非法（mod 4 == 1）。
        assert!(decode("A").is_err());
        assert!(decode("Zg=").is_err());
        assert!(decode("A==").is_err());
        // 填充数量非法。
        assert!(decode("====").is_err());
        assert!(decode("Zg===").is_err());
        assert!(decode("Zm9vYg===").is_err());
        // 非法字符。
        assert!(decode("Zg!=").is_err());
        assert!(decode("-_-_").is_err());
        // 中间不允许出现填充。
        assert!(decode("Z=g=").is_err());
    }
}
