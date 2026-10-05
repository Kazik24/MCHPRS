//! Velocity modern forwarding. Verify the MAC before interpreting any identity.
use anyhow::{bail, ensure, Context, Result};
use hmac::{Hmac, Mac};
use mchprs_network::packets::clientbound::CPlayerInfoAddPlayerProperty;
use mchprs_network::packets::PacketDecoderExt;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::io::Cursor;
use std::net::IpAddr;
use std::path::PathBuf;

static SECRET: OnceCell<Vec<u8>> = OnceCell::new();

#[derive(Serialize, Deserialize)]
pub struct VelocityConfig {
    pub secret_file: PathBuf,
}

pub fn init(config: &VelocityConfig) -> Result<()> {
    let secret = std::fs::read_to_string(&config.secret_file)
        .context("Cannot read Velocity forwarding secret file")?;
    let secret = secret.trim().as_bytes().to_vec();
    ensure!(
        secret.len() >= 32,
        "Velocity forwarding secret must have at least 32 bytes"
    );
    SECRET
        .set(secret)
        .map_err(|_| anyhow::anyhow!("Velocity already initialized"))
}

pub struct ForwardedProfile {
    pub uuid: u128,
    pub properties: Vec<CPlayerInfoAddPlayerProperty>,
}

pub fn verify(data: &[u8], username: &str) -> Result<ForwardedProfile> {
    verify_with_secret(
        SECRET.get().context("Velocity is not initialized")?,
        data,
        username,
    )
}

fn verify_with_secret(secret: &[u8], data: &[u8], username: &str) -> Result<ForwardedProfile> {
    ensure!(
        (32..=32768).contains(&data.len()),
        "Invalid forwarding payload length"
    );
    let (signature, payload) = data.split_at(32);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).context("Invalid forwarding secret")?;
    mac.update(payload);
    mac.verify_slice(signature)
        .map_err(|_| anyhow::anyhow!("Invalid forwarding signature"))?;

    let mut reader = Cursor::new(payload);
    let decode = || -> Result<ForwardedProfile> { bail!("Malformed forwarding payload") };
    let version = reader
        .read_varint()
        .map_err(|_| anyhow::anyhow!("Missing forwarding version"))?;
    ensure!(version == 1, "Unsupported forwarding version");
    let ip = reader
        .read_string()
        .map_err(|_| anyhow::anyhow!("Missing forwarded address"))?;
    ensure!(ip.parse::<IpAddr>().is_ok(), "Invalid forwarded address");
    let uuid = reader
        .read_uuid()
        .map_err(|_| anyhow::anyhow!("Missing forwarded UUID"))?;
    ensure!(uuid != 0, "Invalid forwarded UUID");
    let name = reader
        .read_string()
        .map_err(|_| anyhow::anyhow!("Missing forwarded name"))?;
    ensure!(
        name == username && valid_username(&name),
        "Forwarded name does not match login"
    );
    let count = reader
        .read_varint()
        .map_err(|_| anyhow::anyhow!("Missing profile properties"))?;
    ensure!((0..=16).contains(&count), "Too many profile properties");
    let mut properties = Vec::new();
    for _ in 0..count {
        let Ok(name) = reader.read_string() else {
            return decode();
        };
        let Ok(value) = reader.read_string() else {
            return decode();
        };
        ensure!(
            name.len() <= 64 && value.len() <= 16384,
            "Profile property too large"
        );
        let signature = match reader.read_unsigned_byte() {
            Ok(0) => None,
            Ok(1) => {
                let Ok(signature) = reader.read_string() else {
                    return decode();
                };
                ensure!(signature.len() <= 4096, "Profile signature too large");
                Some(signature)
            }
            _ => return decode(),
        };
        properties.push(CPlayerInfoAddPlayerProperty {
            name,
            value,
            signature,
        });
    }
    ensure!(
        reader.position() as usize == payload.len(),
        "Unexpected forwarding data"
    );
    Ok(ForwardedProfile { uuid, properties })
}

pub fn valid_username(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use mchprs_network::packets::PacketEncoderExt;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";
    fn payload() -> Vec<u8> {
        let mut data = Vec::new();
        data.write_varint(1);
        data.write_string(32767, "127.0.0.1");
        data.write_uuid(1234);
        data.write_string(16, "VerifiedUser");
        data.write_varint(1);
        data.write_string(32767, "textures");
        data.write_string(32767, "skin-and-cape");
        data.write_bool(true);
        data.write_string(32767, "mojang-signature");
        data
    }
    fn sign(payload: &[u8]) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(KEY).unwrap();
        mac.update(payload);
        let mut data = mac.finalize().into_bytes().to_vec();
        data.extend_from_slice(payload);
        data
    }
    #[test]
    fn authenticates_identity_and_preserves_signed_skins() {
        let profile = verify_with_secret(KEY, &sign(&payload()), "VerifiedUser").unwrap();
        assert_eq!(profile.uuid, 1234);
        assert_eq!(profile.properties[0].name, "textures");
        assert_eq!(profile.properties[0].value, "skin-and-cape");
        assert_eq!(
            profile.properties[0].signature.as_deref(),
            Some("mojang-signature")
        );
    }
    #[test]
    fn rejects_spoofing_bad_signatures_and_wrong_names() {
        let mut signed = sign(&payload());
        signed[40] ^= 1;
        assert!(verify_with_secret(KEY, &signed, "VerifiedUser").is_err());
        assert!(
            verify_with_secret(b"different-secret", &sign(&payload()), "VerifiedUser").is_err()
        );
        assert!(verify_with_secret(KEY, &sign(&payload()), "Impersonator").is_err());
        assert!(verify_with_secret(KEY, &[], "VerifiedUser").is_err());
    }
    #[test]
    fn rejects_authenticated_but_malformed_profiles() {
        let original = payload();
        for n in 0..original.len() {
            assert!(verify_with_secret(KEY, &sign(&original[..n]), "VerifiedUser").is_err());
        }
        let mut data = original.clone();
        data[0] = 4;
        assert!(verify_with_secret(KEY, &sign(&data), "VerifiedUser").is_err());
        let mut data = original;
        data.push(0);
        assert!(verify_with_secret(KEY, &sign(&data), "VerifiedUser").is_err());
        assert!(!valid_username("a\0b"));
        assert!(!valid_username(""));
    }
}
