use super::*;

// ---- signature ---------------------------------------------------------------------------

/// RFC 4231, test cases 1, 2 and 6 (key longer than the block size).
#[test]
fn hmac_matches_the_rfc_vectors() {
    assert_eq!(
        hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hex(&hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn signature_valid_cases() {
    let body = br#"{"a":1}"#;
    let good = signature_header(b"secret", body);
    assert!(signature_valid(b"secret", body, Some(&good)));
    // Hex in upper case is the same number.
    let upper = format!(
        "sha256={}",
        good.trim_start_matches("sha256=").to_uppercase()
    );
    assert!(signature_valid(b"secret", body, Some(&upper)));
    // Wrong secret, other body, missing header.
    assert!(!signature_valid(b"other", body, Some(&good)));
    assert!(!signature_valid(b"secret", br#"{"a":2}"#, Some(&good)));
    assert!(!signature_valid(b"secret", body, None));
    // Wrong prefix, wrong length, not hex, empty.
    assert!(!signature_valid(
        b"secret",
        body,
        Some(good.trim_start_matches("sha256="))
    ));
    assert!(!signature_valid(b"secret", body, Some("sha1=abcd")));
    assert!(!signature_valid(
        b"secret",
        body,
        Some(&good[..good.len() - 2])
    ));
    assert!(!signature_valid(
        b"secret",
        body,
        Some(&format!("{good}00"))
    ));
    assert!(!signature_valid(
        b"secret",
        body,
        Some(&format!("sha256={}", "zz".repeat(32)))
    ));
    assert!(!signature_valid(b"secret", body, Some("sha256=")));
    assert!(!signature_valid(b"secret", body, Some("")));
}

#[test]
fn constant_time_compare() {
    assert!(equal_constant_time(b"abc", b"abc"));
    assert!(!equal_constant_time(b"abc", b"abd"));
    assert!(!equal_constant_time(b"abc", b"abcd"));
}

#[test]
fn generated_secrets_are_long_and_different() {
    let a = secret_generate().unwrap();
    let b = secret_generate().unwrap();
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}
