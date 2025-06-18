use xxhash_rust::xxh3::xxh3_64;

/// Generate hash of the buffer.
#[inline]
pub fn ut_fold_string(buf: &[u8]) -> u32 {
    (xxh3_64(buf) & 0xFFFF_FFFF) as u32
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
