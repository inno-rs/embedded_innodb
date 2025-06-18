#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rstest::{fixture, rstest};

    use crate::fil::fil0fil::Fil;
    use crate::innodb::innodb0types::{
        IB_FILE_BLOCK_SIZE, Lsn, UNIV_PAGE_SIZE,
    };
    use crate::log::log0log::Log;
    use crate::log::log0types::{
        LOG_BLOCK_HDR_SIZE, LOG_BLOCK_TRL_SIZE, LOG_START_LSN,
    };
    use crate::os::os0io::OsIo;
    use crate::os::os0runtime::create_runtime;

    #[fixture]
    fn log() -> Log {
        let runtime = create_runtime(1);
        let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
        let fil = Arc::new(Mutex::new(Fil::new(100, os_io.clone(), runtime)));

        Log::new(UNIV_PAGE_SIZE * 10, fil, Default::default())
    }

    #[rstest]
    fn test_block_convert_lsn_to_no(log: Log) {
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

    #[rstest]
    fn test_block_store_checksum(mut log: Log) {
        let block_offset = 0;
        let checksum = log.block_calc_checksum(block_offset);

        log.block_store_checksum(block_offset);

        let retrieved_checksum = log.block_get_checksum(block_offset);

        // Check if the checksum is stored correctly
        assert_eq!(checksum, retrieved_checksum);
    }

    #[rstest]
    fn test_block_set_flush_bit(mut log: Log) {
        let block_offset = IB_FILE_BLOCK_SIZE;

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

    #[rstest]
    fn test_block_set_header_no(mut log: Log) {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let hdr_no = 89;

        log.block_set_hdr_no(block_offset, hdr_no);

        // Check if the header number is set correctly
        assert_eq!(log.block_get_hdr_no(block_offset), hdr_no);
    }

    #[rstest]
    fn test_block_set_data_length(mut log: Log) {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let data_length = 100;

        log.block_set_data_length(block_offset, data_length);

        // Check if the data length is set correctly
        assert_eq!(log.block_get_data_length(block_offset), data_length);
    }

    #[rstest]
    fn test_block_set_first_rec_group(mut log: Log) {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let first_rec_group = 200;

        log.block_set_first_rec_group(block_offset, first_rec_group);

        // Check if the first record group is set correctly
        assert_eq!(
            log.block_get_first_rec_group(block_offset),
            first_rec_group
        );
    }

    #[rstest]
    fn test_block_set_checkpoint_no(mut log: Log) {
        let block_offset = IB_FILE_BLOCK_SIZE;
        let checkpoint_no = 300;

        log.block_set_checkpoint_number(block_offset, checkpoint_no);

        // Check if the checkpoint number is set correctly
        assert_eq!(
            log.block_get_checkpoint_number(block_offset),
            checkpoint_no
        );
    }

    #[rstest]
    fn test_block_init(mut log: Log) {
        let block_offset = IB_FILE_BLOCK_SIZE;
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

    #[rstest]
    fn test_reserve_and_write_fast_small_data(mut log: Log) {
        let input1 = b"hello world!";
        let input2 = b"bye bye world!";
        {
            let ret = log.reserve_and_write_fast(&mut input1.to_vec());
            assert!(ret.is_ok());

            let log_write_result = ret.unwrap();
            assert_eq!(log_write_result.start_lsn, LOG_START_LSN as u64);
            assert_eq!(
                log_write_result.end_lsn,
                LOG_START_LSN as u64 + input1.to_vec().len() as u64
            );
        }

        {
            let ret = log.reserve_and_write_fast(&mut input2.to_vec());
            assert!(ret.is_ok());

            let log_write_result = ret.unwrap();
            assert_eq!(
                log_write_result.start_lsn,
                LOG_START_LSN + input1.len() as Lsn
            );
            assert_eq!(
                log_write_result.end_lsn,
                input1.to_vec().len() as Lsn
                    + input2.to_vec().len() as Lsn
                    + LOG_START_LSN
            );
        }
    }

    #[rstest]
    fn test_write_log(mut log: Log) {
        let mut input1 = b"hello world!".to_vec();
        input1.resize(IB_FILE_BLOCK_SIZE, 'a' as u8);
        let mut input2 = b"bye bye world!".to_vec();
        input2.resize(IB_FILE_BLOCK_SIZE, 'a' as u8);

        {
            let ret = log.write_low(&mut input1);
            assert!(ret.is_ok());

            let log_write_result = ret.unwrap();
            assert_eq!(log_write_result.start_lsn, LOG_START_LSN as u64);
            assert_eq!(
                log_write_result.end_lsn,
                LOG_START_LSN as Lsn
                    + input1.to_vec().len() as Lsn
                    + LOG_BLOCK_HDR_SIZE as Lsn
                    + LOG_BLOCK_TRL_SIZE as Lsn
            );
        }

        {
            let ret = log.write_low(&mut input2.to_vec());
            assert!(ret.is_ok());

            let log_write_result = ret.unwrap();
            assert_eq!(
                log_write_result.start_lsn,
                LOG_START_LSN
                    + input1.to_vec().len() as Lsn
                    + LOG_BLOCK_HDR_SIZE as Lsn
                    + LOG_BLOCK_TRL_SIZE as Lsn
            );
            assert_eq!(
                log_write_result.end_lsn,
                input1.to_vec().len() as Lsn
                    + input2.to_vec().len() as Lsn
                    + (LOG_BLOCK_HDR_SIZE + LOG_BLOCK_TRL_SIZE) as Lsn * 2
                    + LOG_START_LSN as Lsn
            );
        }
    }

    #[rstest]
    fn test_reserve_and_write_fast_big_data(mut log: Log) {
        let mut data = b"hello world!".to_vec();
        data.resize(IB_FILE_BLOCK_SIZE, 0u8);
        let ret = log.reserve_and_write_fast(&mut data);
        assert!(ret.is_err());
    }

    #[rstest]
    fn test_group_get_capacity(log: Log) {
        let capacity = log.group_get_capacity();
        assert_eq!(capacity, 6_549_504);
    }

    #[rstest]
    fn test_group_calc_size_offset(log: Log) {
        // Does not work for 0. Why??

        let val = log.group_calc_size_offset(1000);
        assert_eq!(val, 999);

        let val = log.group_calc_size_offset(2453235235);
        assert_eq!(val, 2453234486);
    }

    #[rstest]
    fn test_group_calc_real_offset(log: Log) {
        let val = log.group_calc_real_offset(0);
        assert_eq!(val, 2048);

        let val = log.group_calc_real_offset(100);
        assert_eq!(val, 2148);

        let val = log.group_calc_real_offset(10000);
        assert_eq!(val, 12048);

        let val = log.group_calc_real_offset(2453235235);
        assert_eq!(val, 2454771235);
    }
}
