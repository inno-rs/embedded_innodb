use rand::{RngCore, SeedableRng, rngs::StdRng};
use xxhash_rust::xxh3::xxh3_64;

/// Generate hash of the buffer.
#[inline]
pub fn ut_fold_string(buf: &[u8]) -> u32 {
    (xxh3_64(buf) & 0xFFFF_FFFF) as u32
}

/// Generate a pseudo-random number based on a seed.
///
/// # Arguments
///
/// * `seed` - A seed value for the random number generator.
///
/// # Returns
///
/// A pseudo-random number generated from the seed.
pub fn ut_next_seed(seed: u64) -> u64 {
    let mut rng = StdRng::seed_from_u64(seed);
    rng.next_u64()
}

#[cfg(test)]
mod tests {
    use crate::ut::ut0rnd::ut_fold_string;

    #[test]
    fn test_ut_fold_string() {
        let input = b"hello world!";
        let out = ut_fold_string(input);
        assert_eq!(out, 1921993496);
    }
}
