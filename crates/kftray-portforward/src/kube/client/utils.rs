use anyhow::{
    Context,
    Result,
};
use openssl::pkey::{
    Id,
    PKey,
};

pub fn is_pkcs8_key(key_data: &[u8]) -> bool {
    const PKCS8_HEADER: &[u8] = b"-----BEGIN PRIVATE KEY-----";
    key_data.len() >= PKCS8_HEADER.len() && key_data.starts_with(PKCS8_HEADER)
}

pub fn convert_pkcs8_to_pkcs1(pkcs8_key: &[u8]) -> Result<Option<Vec<u8>>> {
    let pkey = PKey::private_key_from_pem(pkcs8_key).context("Failed to parse PKCS#8 key")?;
    if pkey.id() != Id::RSA {
        return Ok(None);
    }
    let rsa = pkey.rsa().context("Failed to extract RSA key from PKey")?;
    let pkcs1_key = rsa
        .private_key_to_pem()
        .context("Failed to convert to PKCS#1")?;
    Ok(Some(pkcs1_key))
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

        assert!(!is_pkcs8_key(b""));
        assert!(!is_pkcs8_key(b"-----"));
        assert!(!is_pkcs8_key(b"-----BEGIN"));
        assert!(!is_pkcs8_key(b"-----BEGIN PRIVATE"));
    }

    #[test]
    fn test_convert_pkcs8_to_pkcs1_converts_rsa_key() {
        use openssl::rsa::Rsa;

        let rsa = Rsa::generate(2048).unwrap();
        let pkcs8_key = PKey::from_rsa(rsa.clone())
            .unwrap()
            .private_key_to_pem_pkcs8()
            .unwrap();

        let pkcs1_key = convert_pkcs8_to_pkcs1(&pkcs8_key).unwrap().unwrap();

        assert!(pkcs1_key.starts_with(b"-----BEGIN RSA PRIVATE KEY-----"));
        let converted = Rsa::private_key_from_pem(&pkcs1_key).unwrap();
        assert_eq!(converted.n().to_vec(), rsa.n().to_vec());
        assert_eq!(converted.d().to_vec(), rsa.d().to_vec());
    }

    #[test]
    fn test_convert_pkcs8_to_pkcs1_leaves_non_rsa_keys() {
        use openssl::ec::{
            EcGroup,
            EcKey,
        };
        use openssl::nid::Nid;

        let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
        let ec_key = PKey::from_ec_key(EcKey::generate(&group).unwrap())
            .unwrap()
            .private_key_to_pem_pkcs8()
            .unwrap();
        let ed25519_key = PKey::generate_ed25519()
            .unwrap()
            .private_key_to_pem_pkcs8()
            .unwrap();

        assert_eq!(convert_pkcs8_to_pkcs1(&ec_key).unwrap(), None);
        assert_eq!(convert_pkcs8_to_pkcs1(&ed25519_key).unwrap(), None);
    }
}
