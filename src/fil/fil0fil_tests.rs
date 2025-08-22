#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rstest::{fixture, rstest};
    use uuid::Uuid;

    use crate::{
        fil::{
            fil0fil::Fil,
            fil0types::{FIL_IBD_FILE_INITIAL_SIZE, FilType},
        },
        innodb::innodb0types::{
            DbErr::DbTablespaceDeleted, IB_FILE_BLOCK_SIZE, PageFrame,
            UNIV_PAGE_SIZE,
        },
        os::{
            os0file::{IORequest, os_file_get_size},
            os0io::OsIo,
            os0runtime::create_runtime,
        },
        some_or_panic,
    };

    #[fixture]
    fn runtime() -> Arc<tokio::runtime::Runtime> {
        create_runtime(1)
    }

    #[fixture]
    fn os_io(runtime: Arc<tokio::runtime::Runtime>) -> Arc<Mutex<OsIo>> {
        Arc::new(Mutex::new(OsIo::new(runtime)))
    }

    #[fixture]
    fn fil(
        runtime: Arc<tokio::runtime::Runtime>,
        os_io: Arc<Mutex<OsIo>>,
    ) -> Fil {
        Fil::new(10, os_io, runtime)
    }

    #[rstest]
    fn test_create_fil_space(mut fil: Fil) {
        let result = fil.space_create(
            &"test".to_string(),
            10,
            0,
            FilType::FilTablespace,
        );

        // FilSpace is empty. Creating a new one should return true.
        assert!(result);

        // Adding the same name again should fail.
        {
            let result = fil.space_create(
                &"test".to_string(),
                10,
                0,
                FilType::FilTablespace,
            );

            assert!(!result);
        }

        // Adding a different name but the same space_id should fail as well
        {
            let result = fil.space_create(
                &"test_2".to_string(),
                10,
                0,
                FilType::FilTablespace,
            );

            assert!(!result);
        }

        // Adding a totally different name and space_id should succeed.

        {
            let result = fil.space_create(
                &"test_2".to_string(),
                11,
                0,
                FilType::FilTablespace,
            );

            assert!(result);
        }
    }

    #[rstest]
    fn test_get_space_by_name(mut fil: Fil) {
        // No space is present, the result should be None.
        let file_name = "test";
        let result = fil.space_get_by_name(file_name);
        assert!(result.is_none());

        // Create a space
        let fil_space_result = fil.space_create(
            &file_name.to_string(),
            20,
            20,
            FilType::FilTablespace,
        );
        assert!(fil_space_result);

        // Now we should be able to retrieve it
        let result = fil.space_get_by_name(file_name);
        assert!(result.is_some());
    }

    #[rstest]
    fn test_space_get_by_id(mut fil: Fil) {
        // No space is present, the result should be None.
        let space_id = 20;
        let result = fil.space_get_by_id(space_id);
        assert!(result.is_none());

        // Create a space
        let file_name = "test";
        let fil_space_result = fil.space_create(
            &file_name.to_string(),
            space_id,
            20,
            FilType::FilTablespace,
        );
        assert!(fil_space_result);

        // Now we should be able to retrieve it
        let result = fil.space_get_by_id(space_id);
        assert!(result.is_some());
    }

    #[rstest]
    fn test_node_create(mut fil: Fil) {
        // No space is present, the result should be false.
        let file_name = "test";
        let result = fil.node_create(file_name, 100, 20, false);
        assert!(!result);

        let fil_space_result = fil.space_create(
            &file_name.to_string(),
            20,
            20,
            FilType::FilTablespace,
        );
        assert!(fil_space_result);

        // Retry creating fil_node
        let result = fil.node_create(file_name, 100, 20, false);
        assert!(result);
    }

    #[rstest]
    fn test_space_get_version(mut fil: Fil) {
        // No space is present, the result should be None.
        let file_name = "test";
        let space_id = 20;
        let result = fil.space_get_version(space_id);
        assert!(result.is_none());

        // Create a space
        let fil_space_result = fil.space_create(
            &file_name.to_string(),
            space_id,
            20,
            FilType::FilTablespace,
        );
        assert!(fil_space_result);

        // Now we should be able to retrieve the version
        let result = fil.space_get_version(space_id);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 1);
    }

    #[rstest]
    fn test_tablespace_version_increments(mut fil: Fil) {
        // Check initial version
        let initial_version = fil.space_get_version(0);
        assert!(initial_version.is_none());

        let mut version_id = 1;
        for space_id in 0..10 {
            let fil_space_result = fil.space_create(
                &format!("test_{}", space_id).to_string(),
                space_id,
                20,
                FilType::FilTablespace,
            );
            assert!(fil_space_result);

            // Check the new version
            let new_version = fil.space_get_version(space_id).unwrap();
            assert_eq!(new_version, version_id);
            version_id += 1;
        }
    }

    #[rstest]
    #[case(FilType::FilTablespace, "test", 20)]
    #[case(FilType::FilLog, "log_space", 21)]
    fn test_space_get_type(
        mut fil: Fil,
        #[case] fil_type: FilType,
        #[case] file_name: &str,
        #[case] space_id: u32,
    ) {
        // No space is present, the result should be None.
        let result = fil.space_get_type(space_id);
        assert!(result.is_none());

        // Create a space
        let fil_space_result =
            fil.space_create(&file_name.to_string(), space_id, 20, fil_type);
        assert!(fil_space_result);

        // Now we should be able to retrieve the type
        let result = fil.space_get_type(space_id);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), fil_type);
    }

    #[rstest]
    fn test_space_free(mut fil: Fil) {
        // No space is present, the result should be false.
        let file_name = "test";

        let fil_space_result = fil.space_create(
            &file_name.to_string(),
            20,
            20,
            FilType::FilTablespace,
        );
        assert!(fil_space_result);

        // Retry creating fil_node
        let result = fil.node_create(file_name, 100, 20, false);
        assert!(result);
        assert_eq!(fil.unflushed_space_list_len(), 1);

        let result = fil.space_free(20);
        assert!(result);

        assert_eq!(fil.unflushed_space_list_len(), 0);
    }

    #[rstest]
    fn test_io_read_with_no_filespace(mut fil: Fil) {
        let mut data = b"Hello, world!".to_vec();
        data.resize(UNIV_PAGE_SIZE, 0);
        let page_frame =
            PageFrame::new(data.as_mut_slice().try_into().unwrap());

        let result = fil.io(
            IORequest::SyncRead,
            false,
            0,
            0,
            100,
            page_frame.0,
            IB_FILE_BLOCK_SIZE,
        );

        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), DbTablespaceDeleted);
    }

    #[rstest]
    fn test_io_read_with_filespace(mut fil: Fil) {
        let file_name = format!("{}_{}", "test", Uuid::new_v4());
        let space_id = 20;
        fil.create_new_single_table_space(
            space_id,
            &file_name,
            0,
            FIL_IBD_FILE_INITIAL_SIZE,
        )
        .expect("Failed to create new single table space");

        assert!(fil.space_get_file_node_count(space_id) > 0);

        let mut data = b"Hello, world!".to_vec();
        data.resize(UNIV_PAGE_SIZE, 0);
        let page_frame =
            PageFrame::new(data.as_mut_slice().try_into().unwrap());

        // Perform a read operation
        let read_result = fil.io(
            IORequest::SyncRead,
            false,
            space_id,
            0,
            0,
            page_frame.0,
            IB_FILE_BLOCK_SIZE,
        );

        assert!(read_result.is_ok());
    }

    #[rstest]
    #[case("test_file", "/tmp/test_file.ibd")]
    #[case("another_test", "/tmp/another_test.ibd")]
    #[case("data_file", "/tmp/data_file.ibd")]
    fn test_make_fil_name(
        fil: Fil,
        #[case] file_name: &str,
        #[case] expected: &str,
    ) {
        // Test with a valid file name
        let result = fil.make_ibd_filename(file_name);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_extend_space_to_desired_size(mut fil: Fil) {
        // prep
        let file_name = format!("{}_{}", "test", Uuid::new_v4());
        let space_id = 20;
        fil.create_new_single_table_space(
            space_id,
            &file_name,
            0,
            FIL_IBD_FILE_INITIAL_SIZE,
        )
        .expect("Failed to create new single table space");

        // call and assert
        assert!(fil.extend_space_to_desired_size(0, space_id, 10));

        let result = some_or_panic!(fil.space_get_by_id(space_id));
        let fil_space = result.read().unwrap().fil_space.clone();
        let fil_space_borrow = fil_space.borrow();
        let last_fil_node_container =
            some_or_panic!(fil_space_borrow.chain.back().get());
        let fil_node = last_fil_node_container.fil_node.clone();
        let fil_node_borrow = fil_node.borrow();
        let os_file = some_or_panic!(&fil_node_borrow.f_handle);

        let new_size = (UNIV_PAGE_SIZE * 10) as i64;
        assert_eq!(new_size, os_file_get_size(os_file))
    }
}
