use anyhow::{
    Context,
    Result,
};
use openssl::pkey::PKey;

pub fn is_pkcs8_key(key_data: &[u8]) -> bool {
    const PKCS8_HEADER: &[u8] = b"-----BEGIN PRIVATE KEY-----";
    key_data.len() >= PKCS8_HEADER.len() && key_data.starts_with(PKCS8_HEADER)
}

pub fn convert_pkcs8_to_pkcs1(pkcs8_key: &[u8]) -> Result<Vec<u8>> {
    let pkey = PKey::private_key_from_pem(pkcs8_key).context("Failed to parse PKCS#8 key")?;
    let rsa = pkey.rsa().context("Failed to extract RSA key from PKey")?;
    let pkcs1_key = rsa
        .private_key_to_pem()
        .context("Failed to convert to PKCS#1")?;
    Ok(pkcs1_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_pkcs8_key() {
        assert!(is_pkcs8_key(
            b"-----BEGIN PRIVATE KEY-----\ndata\n-----END PRIVATE KEY-----"
        ));
        assert!(!is_pkcs8_key(
            b"-----BEGIN RSA PRIVATE KEY-----\ndata\n-----END RSA PRIVATE KEY-----"
        ));
        assert!(!is_pkcs8_key(b"random data"));

        // Test edge cases for buffer overflow protection
        assert!(!is_pkcs8_key(b""));
        assert!(!is_pkcs8_key(b"-----"));
        assert!(!is_pkcs8_key(b"-----BEGIN"));
        assert!(!is_pkcs8_key(b"-----BEGIN PRIVATE"));
    }
}
