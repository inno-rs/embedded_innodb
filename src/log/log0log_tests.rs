#[cfg(test)]
mod tests {
    use crate::innodb::innodb0types::{IB_FILE_BLOCK_SIZE, Lsn};
    use crate::log::log0log::Log;
    use crate::log::log0types::LOG_BLOCK_HDR_SIZE;

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

    #[test]
    fn test_block_set_flush_bit() {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE * 2);

        {
            log.block_set_flush_bit(block_offset, true);

            // Check if the flush bit is set correctly
            assert_eq!(log.block_get_flush_bit(block_offset), true);
        }

        {
            log.block_set_flush_bit(block_offset, false);

            // Check if the flush bit is set correctly
            assert_eq!(log.block_get_flush_bit(block_offset), false);
        }
    }

    #[test]
    fn test_block_set_header_no() {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE * 2);
        let hdr_no = 89;

        log.block_set_hdr_no(block_offset, hdr_no);

        // Check if the header number is set correctly
        assert_eq!(log.block_get_hdr_no(block_offset), hdr_no);
    }

    #[test]
    fn test_block_set_data_length() {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE * 2);
        let data_length = 100;

        log.block_set_data_length(block_offset, data_length);

        // Check if the data length is set correctly
        assert_eq!(log.block_get_data_length(block_offset), data_length);
    }

    #[test]
    fn test_block_set_first_rec_group() {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE * 2);
        let first_rec_group = 200;

        log.block_set_first_rec_group(block_offset, first_rec_group);

        // Check if the first record group is set correctly
        assert_eq!(
            log.block_get_first_rec_group(block_offset),
            first_rec_group
        );
    }

    #[test]
    fn test_block_set_checkpoint_no() {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE * 2);
        let checkpoint_no = 300;

        log.block_set_checkpoint_number(block_offset, checkpoint_no);

        // Check if the checkpoint number is set correctly
        assert_eq!(
            log.block_get_checkpoint_number(block_offset),
            checkpoint_no
        );
    }

    #[test]
    fn test_block_init() {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let mut log = Log::new(IB_FILE_BLOCK_SIZE * 2);
        let lsn = (IB_FILE_BLOCK_SIZE + 34) as Lsn;
        let block_no = Log::block_convert_lsn_to_no(lsn);

        log.block_init(block_offset, lsn);

        // Check if the block is initialized correctly
        assert_eq!(log.block_get_hdr_no(block_offset), block_no);
        assert_eq!(
            log.block_get_data_length(block_offset),
            LOG_BLOCK_HDR_SIZE
        );
        assert_eq!(log.block_get_first_rec_group(block_offset), 0);
        assert_eq!(log.block_get_checkpoint_number(block_offset), 0);
    }
}
