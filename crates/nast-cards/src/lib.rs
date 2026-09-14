//! PNG tEXt chunk 读写 + 卡片 JSON 提取/嵌入。
//!
//! 行为契约（对齐 refrence/SillyTavern/src/character-card-parser.js）：
//! - 只识别未压缩 tEXt chunk（png-chunk-text 语义），忽略 zTXt/iTXt
//! - keyword：`chara`（V1/V2，base64(UTF-8 JSON)）与 `ccv3`（V3），两者同时存在时 V3 优先
//! - 写入：重编 chunk 保留其余原样，插入 tEXt；CRC 重算

use base64::Engine as _;
use nast_model::card::Character;
use nast_model::{ModelError, ModelResult};
use std::collections::BTreeMap;

pub const KEY_CHARA: &str = "chara";
pub const KEY_CCV3: &str = "ccv3";

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardSpec {
    V1,
    V2,
    V3,
}

/// 一个 PNG 文件的完整 chunk 列表。
struct Chunk {
    kind: [u8; 4],
    data: Vec<u8>,
}

/// CRC32（PNG 标准 zlib 多项式 0xEDB88320）。
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB88320 & mask);
        }
    }
    !crc
}

/// 解析全部 chunk（IHDR..IEND，不解码图像）。
fn parse_chunks(buf: &[u8]) -> ModelResult<Vec<Chunk>> {
    if buf.len() < 8 || buf[..8] != PNG_SIG {
        return Err(ModelError::InvalidValue { field: "png", reason: "bad signature".into() });
    }
    let mut out = Vec::new();
    let mut pos = 8usize;
    while pos + 8 <= buf.len() {
        let len = u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
        let kind = [buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]];
        let data_start = pos + 8;
        let data_end = data_start.checked_add(len).ok_or(ModelError::InvalidValue {
            field: "png",
            reason: "chunk length overflow".into(),
        })?;
        if data_end + 4 > buf.len() {
            return Err(ModelError::InvalidValue { field: "png", reason: "truncated chunk".into() });
        }
        out.push(Chunk { kind, data: buf[data_start..data_end].to_vec() });
        pos = data_end + 4; // skip CRC
        if &kind == b"IEND" {
            break;
        }
    }
    Ok(out)
}

/// 序列化 chunks 为 PNG 字节流。
fn write_chunks(chunks: &[Chunk]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&PNG_SIG);
    for c in chunks {
        out.extend_from_slice(&(c.data.len() as u32).to_be_bytes());
        out.extend_from_slice(&c.kind);
        let mut crc_input = Vec::with_capacity(4 + c.data.len());
        crc_input.extend_from_slice(&c.kind);
        crc_input.extend_from_slice(&c.data);
        out.extend_from_slice(&c.data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }
    out
}

/// 解析 tEXt：keyword\0text。
fn parse_text_chunk(data: &[u8]) -> Option<(String, Vec<u8>)> {
    let nul = data.iter().position(|&b| b == 0)?;
    let keyword: String = data[..nul].iter().map(|&b| b as char).collect();
    Some((keyword, data[nul + 1..].to_vec()))
}

/// 构造 tEXt chunk 数据。
fn build_text_chunk(keyword: &str, text: &[u8]) -> Chunk {
    let mut data = Vec::with_capacity(keyword.len() + 1 + text.len());
    data.extend_from_slice(keyword.as_bytes());
    data.push(0);
    data.extend_from_slice(text);
    Chunk { kind: *b"tEXt", data }
}

/// 提取卡片 JSON（ccv3 优先于 chara）。
pub fn extract_card_json(png: &[u8]) -> ModelResult<(serde_json::Value, CardSpec)> {
    let chunks = parse_chunks(png)?;
    let mut found: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
    for c in &chunks {
        if &c.kind == b"tEXt" {
            if let Some((kw, val)) = parse_text_chunk(&c.data) {
                if kw == KEY_CHARA {
                    found.entry(KEY_CHARA).or_insert(val);
                } else if kw == KEY_CCV3 {
                    found.entry(KEY_CCV3).or_insert(val);
                }
            }
        }
    }
    let (kw, raw) = if let Some(v) = found.get(KEY_CCV3) {
        (KEY_CCV3, v)
    } else if let Some(v) = found.get(KEY_CHARA) {
        (KEY_CHARA, v)
    } else {
        return Err(ModelError::MissingField("chara/ccv3 tEXt chunk"));
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|_| ModelError::InvalidValue { field: kw, reason: "card chunk is not base64".into() })?;
    let json: serde_json::Value = serde_json::from_slice(&decoded)?;
    let spec = match (kw, json.get("spec").and_then(|s| s.as_str())) {
        (KEY_CCV3, _) => CardSpec::V3,
        (_, Some("chara_card_v2")) => CardSpec::V2,
        (KEY_CHARA, _) if json.get("data").is_some() => CardSpec::V2,
        _ => CardSpec::V1,
    };
    Ok((json, spec))
}

/// 解析为 Character（含 V1 合成）。
pub fn read_card(png: &[u8]) -> ModelResult<Character> {
    let (json, _) = extract_card_json(png)?;
    Character::from_card_json(&json)
}

/// 将卡片 JSON 嵌入 PNG（写 chara；V3 时同时写 ccv3）。
pub fn write_card(
    png: &[u8],
    card_v2: &serde_json::Value,
    card_v3: Option<&serde_json::Value>,
) -> ModelResult<Vec<u8>> {
    let mut chunks = parse_chunks(png)?;
    // ST write 语义（character-card-parser.js:18-27）：先剔除已有的 chara/ccv3 tEXt，避免旧数据遮蔽
    chunks.retain(|c| {
        if &c.kind != b"tEXt" {
            return true;
        }
        match parse_text_chunk(&c.data) {
            Some((kw, _)) => kw != KEY_CHARA && kw != KEY_CCV3,
            None => true,
        }
    });
    let insert_at = chunks
        .iter()
        .position(|c| &c.kind == b"IHDR")
        .map(|i| i + 1)
        .ok_or(ModelError::InvalidValue { field: "png", reason: "missing IHDR".into() })?;
    let b64 = |v: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(v).unwrap_or_default())
    };
    let mut idx = insert_at;
    let mut new_chunks = vec![build_text_chunk(KEY_CHARA, b64(card_v2).as_bytes())];
    if let Some(v3) = card_v3 {
        new_chunks.push(build_text_chunk(KEY_CCV3, b64(v3).as_bytes()));
    }
    for nc in new_chunks {
        chunks.insert(idx, nc);
        idx += 1;
    }
    Ok(write_chunks(&chunks))
}

/// 生成最小合法 PNG（1x1 白点），供新角色初始头像。
pub fn minimal_png() -> Vec<u8> {
    // 预生成的标准 1x1 白色 PNG 字节
    const PNG_1PX: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, //
        0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, //
        0x00, 0x00, 0x00, 0x0C, b'I', b'D', b'A', b'T', 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
        0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, //
        0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];
    PNG_1PX.to_vec()
}

/// 文件名净化（characters.js：非法字符 → _）。
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "unnamed".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_json() -> serde_json::Value {
        serde_json::json!({
            "spec": "chara_card_v2",
            "spec_version": "2.0",
            "name": "Test",
            "data": {"name": "Test", "description": "d"}
        })
    }

    #[test]
    fn roundtrip_write_read() {
        let png = minimal_png();
        let out = write_card(&png, &card_json(), None).unwrap();
        let (json, spec) = extract_card_json(&out).unwrap();
        assert_eq!(spec, CardSpec::V2);
        assert_eq!(json["data"]["description"], "d");
    }

    #[test]
    fn v3_preferred_over_v2() {
        let png = minimal_png();
        let v3 = serde_json::json!({
            "spec": "chara_card_v3",
            "spec_version": "3.0",
            "name": "T3",
            "data": {"name": "T3", "group_only_greetings": []}
        });
        let out = write_card(&png, &card_json(), Some(&v3)).unwrap();
        let (json, spec) = extract_card_json(&out).unwrap();
        assert_eq!(spec, CardSpec::V3);
        assert_eq!(json["spec"], "chara_card_v3");
    }

    #[test]
    fn read_seraphina_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../refrence/SillyTavern/default/content/default_Seraphina.png"
        );
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("fixture missing, skip");
            return;
        };
        let ch = read_card(&bytes).unwrap();
        assert_eq!(ch.name, "Seraphina");
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_filename("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize_filename("  "), "unnamed");
    }
}
