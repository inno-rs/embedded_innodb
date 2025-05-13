#[cfg(test)]
mod tests {
    use crate::innodb::innodb0types::IB_FILE_BLOCK_SIZE;
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

    #[test]
    fn test_block_store_checksum() {
        let block_offset = 0;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE);
        let checksum = log.block_calc_checksum(block_offset);

        log.block_store_checksum(block_offset);

        let retrieved_checksum = log.block_get_checksum(block_offset);

        // Check if the checksum is stored correctly
        assert_eq!(checksum, retrieved_checksum);
    }
}
