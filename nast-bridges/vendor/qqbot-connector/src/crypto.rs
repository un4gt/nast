//! 凭据解密（对应参考实现中的 crypto.go / `decryptSecret`）。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::base64;
use crate::error::{Error, Result};

/// 解密扫码绑定成功后服务端下发的 AES-256-GCM 加密 `appSecret`。
///
/// # 参数
/// - `key_base64`: 调用 [`crate::create_bind_task`] 前生成的 base64 密钥
///   （解码后必须为 32 字节）。
/// - `encrypted_base64`: 服务端返回的 `bot_encrypt_secret`，密文格式为
///   `IV(12 字节) + ciphertext(N 字节) + AuthTag(16 字节)` 的 base64 编码。
///
/// 返回明文 `appSecret`（UTF-8 字符串）。
pub fn decrypt_secret(key_base64: &str, encrypted_base64: &str) -> Result<String> {
    let key = base64::decode(key_base64).map_err(|e| Error::Crypto(format!("decode key: {e}")))?;
    if key.len() != 32 {
        return Err(Error::Crypto(format!(
            "invalid key length: {} (expected 32)",
            key.len()
        )));
    }

    let encrypted = base64::decode(encrypted_base64)
        .map_err(|e| Error::Crypto(format!("decode encrypted: {e}")))?;

    // 最小长度：12 (IV) + 1 (ciphertext) + 16 (tag) = 29
    if encrypted.len() < 29 {
        return Err(Error::Crypto(format!(
            "encrypted data too short: {} bytes",
            encrypted.len()
        )));
    }

    let (iv, rest) = encrypted.split_at(12);
    let (ciphertext, tag) = rest.split_at(rest.len() - 16);

    // RustCrypto 的 GCM 期望 ciphertext 与 tag 拼接在一起。
    let mut sealed = Vec::with_capacity(ciphertext.len() + 16);
    sealed.extend_from_slice(ciphertext);
    sealed.extend_from_slice(tag);

    let cipher = Aes256Gcm::new((&key[..]).into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(iv), sealed.as_ref())
        .map_err(|_| Error::Crypto("GCM authentication failed".into()))?;

    String::from_utf8(plaintext).map_err(|_| Error::Crypto("plaintext is not valid UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    fn b64(bytes: &[u8]) -> String {
        crate::base64::encode(bytes)
    }

    #[test]
    fn nist_gcm_vector_case2() {
        // NIST GCM Test Case 2: 全零 key/IV，明文 16 个 0x00。
        // CT = cea7403d4d606b6e074ec5d3baf39d18
        // Tag = d0d1c8a799996bf0265b98b5d48ab919
        let key = b64(&[0u8; 32]);
        let mut blob = vec![0u8; 12];
        blob.extend_from_slice(&[
            0xce, 0xa7, 0x40, 0x3d, 0x4d, 0x60, 0x6b, 0x6e, 0x07, 0x4e, 0xc5, 0xd3, 0xba, 0xf3,
            0x9d, 0x18,
        ]);
        blob.extend_from_slice(&[
            0xd0, 0xd1, 0xc8, 0xa7, 0x99, 0x99, 0x6b, 0xf0, 0x26, 0x5b, 0x98, 0xb5, 0xd4, 0x8a,
            0xb9, 0x19,
        ]);
        assert_eq!(
            decrypt_secret(&key, &b64(&blob)).unwrap(),
            String::from_utf8(vec![0u8; 16]).unwrap()
        );
    }

    #[test]
    fn roundtrip_with_aes_gcm() {
        // 用 aes-gcm 自身加密构造密文，验证解密路径与真实服务端格式一致。
        let key_bytes: [u8; 32] = core::array::from_fn(|i| (i * 7 + 3) as u8);
        let iv: [u8; 12] = core::array::from_fn(|i| (i * 11 + 1) as u8);
        let plaintext = b"0123456789abcdef-app-secret";

        let cipher = Aes256Gcm::new((&key_bytes).into());
        let sealed = cipher
            .encrypt(Nonce::from_slice(&iv), plaintext.as_ref())
            .unwrap();

        // 服务端格式：IV + ciphertext + tag（sealed 即 ciphertext||tag）。
        let mut blob = iv.to_vec();
        blob.extend_from_slice(&sealed);

        assert_eq!(
            decrypt_secret(&b64(&key_bytes), &b64(&blob)).unwrap(),
            String::from_utf8(plaintext.to_vec()).unwrap()
        );
    }

    #[test]
    fn rejects_bad_input() {
        let key = b64(&[0u8; 32]);
        // 密钥长度错误。
        assert!(matches!(
            decrypt_secret(&b64(&[0u8; 16]), &b64(&[0u8; 29])),
            Err(Error::Crypto(_))
        ));
        // 密文过短。
        assert!(matches!(
            decrypt_secret(&key, &b64(&[0u8; 28])),
            Err(Error::Crypto(_))
        ));
        // base64 非法。
        assert!(matches!(
            decrypt_secret(&key, "@@@@"),
            Err(Error::Crypto(_))
        ));
        // tag 校验失败（篡改密文）。
        assert!(matches!(
            decrypt_secret(&key, &b64(&[0u8; 40])),
            Err(Error::Crypto(_))
        ));
    }
}
