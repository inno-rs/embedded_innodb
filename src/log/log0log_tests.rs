#[cfg(test)]
mod tests {
    use crate::log::log0log::Log;

    #[test]
    fn test_block_convert_lsn_to_no() {
        // It should never be 0.
        {
            let lsn = 5;
            let block_no = Log::block_convert_lsn_to_no(lsn);

            // Check if the conversion is correct
            assert_eq!(block_no, 1);
        }

        {
            let lsn = 512 + 5;
            let block_no = Log::block_convert_lsn_to_no(lsn);

            // Check if the conversion is correct
            assert_eq!(block_no, 2);
        }
    }
}
