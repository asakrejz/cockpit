//! HMAC-SHA256 signatures of webhook bodies.

use super::*;

/// HMAC-SHA256 (RFC 2104) built from SHA-256.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// Compares in constant time (for equal lengths; the length itself is not secret).
pub fn equal_constant_time(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// The header looks like `sha256=<hex>` (hex in any case). Valid if it is the HMAC of `body`.
pub fn signature_valid(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(given) = header
        .and_then(|h| h.trim().strip_prefix("sha256="))
        .and_then(unhex)
    else {
        return false;
    };
    equal_constant_time(&hmac_sha256(secret, body), &given)
}

/// `sha256=<hex>` of the body – what the sender puts in the header.
pub fn signature_header(secret: &[u8], body: &[u8]) -> String {
    format!("sha256={}", hex(&hmac_sha256(secret, body)))
}

/// A fresh random secret: 32 bytes from the system as 64 hex characters.
pub fn secret_generate() -> Result<String, cockpit_i18n::Msg> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|_| cockpit_i18n::msg!(ServerMiscRandomUnavailable))?;
    Ok(hex(&buf))
}
