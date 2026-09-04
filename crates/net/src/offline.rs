//! Offline-mode player identity.

use md5::{Digest, Md5};

use crate::error::NetError;

/// Maximum username length, in characters.
const MAX_USERNAME_CHARS: usize = 16;

/// Derives the offline-mode UUID for a username.
///
/// This is a version 3 UUID per RFC 4122: an MD5 digest over the UTF-8 bytes
/// of `OfflinePlayer:<username>`, with the version nibble and variant bits
/// overwritten. Every implementation that follows the same convention derives
/// the same value, which is what keeps player data, permissions, and world
/// files interchangeable between servers.
///
/// The UUID a client sends in Login Start is deliberately not used: it is
/// unauthenticated, so honouring it would let anyone assume another player's
/// identity by asking.
pub fn offline_uuid(username: &str) -> u128 {
    let mut hasher = Md5::new();
    hasher.update(b"OfflinePlayer:");
    hasher.update(username.as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize().into();

    // Byte 6's high nibble carries the version; set it to 3.
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    // Byte 8's top two bits carry the variant; set them to 0b10 (RFC 4122).
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    u128::from_be_bytes(bytes)
}

/// Checks that a username is one the server will accept.
///
/// One to sixteen characters of `[a-zA-Z0-9_]`. Names outside that set do not
/// round-trip through other tooling and are a cheap way to smuggle odd data
/// into logs and, later, world files -- so they are refused at the door rather
/// than sanitised afterwards.
pub fn validate_username(name: &str) -> Result<(), NetError> {
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_USERNAME_CHARS
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');

    if valid {
        Ok(())
    } else {
        Err(NetError::InvalidUsername {
            name: name.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn offline_uuid_is_stable_for_a_name() {
        assert_eq!(offline_uuid("Notch"), offline_uuid("Notch"));
    }

    #[test]
    fn offline_uuid_matches_the_documented_vanilla_values() {
        // The whole point of this derivation is that other implementations
        // compute the same value, so pin known vectors rather than only
        // asserting self-consistency. A change here breaks interoperability
        // of player data, not just this crate.
        for (name, expected) in [
            ("Notch", 0xb50a_d385_829d_3141_a216_7e7d_7539_ba7fu128),
            ("Steve", 0x5627_dd98_e6be_3c21_b8a8_e923_4418_3641u128),
            ("jeb_", 0xa762_f560_4fce_3236_812a_b80e_fff0_b62bu128),
        ] {
            assert_eq!(offline_uuid(name), expected, "offline uuid for {name}");
        }
    }

    #[test]
    fn offline_uuid_differs_between_names() {
        assert_ne!(offline_uuid("Notch"), offline_uuid("notch"));
        assert_ne!(offline_uuid("Notch"), offline_uuid("Steve"));
    }

    #[test]
    fn offline_uuid_has_version_3_and_rfc_4122_variant() {
        // A UUIDv3 must report version 3 in the high nibble of byte 6 and the
        // RFC 4122 variant in the top bits of byte 8. Clients and other server
        // implementations both check this.
        for name in ["Notch", "Steve", "a", "_______________x"] {
            let bytes = offline_uuid(name).to_be_bytes();
            assert_eq!(bytes[6] >> 4, 0x3, "version nibble for {name}");
            assert_eq!(bytes[8] >> 6, 0b10, "variant bits for {name}");
        }
    }

    #[test]
    fn valid_usernames_are_accepted() {
        for name in ["a", "Notch", "Steve_123", "________________"] {
            assert!(validate_username(name).is_ok(), "{name} should be valid");
        }
    }

    #[test]
    fn invalid_usernames_are_rejected() {
        for name in ["", "seventeen_chars_x", "has space", "hy-phen", "é"] {
            assert!(
                matches!(
                    validate_username(name),
                    Err(NetError::InvalidUsername { .. })
                ),
                "{name:?} should be rejected"
            );
        }
    }
}
