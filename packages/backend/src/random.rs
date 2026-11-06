//! Random bytes for secrets (API keys, visitor salts, test codes), from the
//! operating system's generator through `rand`.

use rand::Rng;

/// `N` random bytes.
pub fn bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// `N` random bytes, hex encoded (`2 * N` characters).
pub fn hex<const N: usize>() -> String {
    hex::encode(bytes::<N>())
}

#[cfg(test)]
mod tests {
    #[test]
    fn differs_every_time() {
        assert_ne!(super::bytes::<32>(), super::bytes::<32>());
        assert_eq!(super::hex::<8>().len(), 16);
    }
}
