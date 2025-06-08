#[cfg(test)]
mod tests {
    use crate::fil::fil0fil::Fil;
    use crate::fil::fil0types::{FilType, FIL_IBD_FILE_INITIAL_SIZE};
    use crate::innodb::innodb0types::DbErr::DbTablespaceDeleted;
    use crate::innodb::innodb0types::{
        PageFrame, IB_FILE_BLOCK_SIZE, UNIV_PAGE_SIZE,
    };
    use crate::os::os0file::IORequest;
    use crate::os::os0io::OsIo;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[test]
    fn test_create_fil_space() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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

    #[test]
    fn test_get_space_by_name() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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

    #[test]
    fn test_space_get_by_id() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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

    #[test]
    fn test_node_create() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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

    #[test]
    fn test_space_get_version() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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

    #[test]
    fn test_tablespace_version_increments() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);
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

    #[test]
    fn test_space_get_type() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

        // No space is present, the result should be None.
        let file_name = "test";
        let space_id = 20;
        let result = fil.space_get_type(space_id);
        assert!(result.is_none());

        {
            // Create a space
            let fil_space_result = fil.space_create(
                &file_name.to_string(),
                space_id,
                20,
                FilType::FilTablespace,
            );
            assert!(fil_space_result);

            // Now we should be able to retrieve the type
            let result = fil.space_get_type(space_id);
            assert!(result.is_some());
            assert_eq!(result.unwrap(), FilType::FilTablespace);
        }

        {
            let space_id = 21;
            // Create a space
            let fil_space_result = fil.space_create(
                &"log_space".to_string(),
                space_id,
                20,
                FilType::FilLog,
            );
            assert!(fil_space_result);

            // Now we should be able to retrieve the type
            let result = fil.space_get_type(space_id);
            assert!(result.is_some());
            assert_eq!(result.unwrap(), FilType::FilLog);
        }
    }

    #[test]
    fn test_space_free() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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

    #[test]
    fn test_io_read_with_no_filespace() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

        let mut data = b"Hello, world!".to_vec();
        data.resize(UNIV_PAGE_SIZE, 0);
        let page_frame = PageFrame(data.as_mut_slice().try_into().unwrap());

        let result = fil.io(
            IORequest::SyncRead,
            false,
            0,
            0,
            100,
            page_frame,
            IB_FILE_BLOCK_SIZE,
        );

        assert!(result.is_err());
        assert_eq!(result.err().unwrap(), DbTablespaceDeleted);
    }

    #[test]
    fn test_io_read_with_filespace() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let mut fil = Fil::new(10, os_io);

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
        let page_frame = PageFrame(data.as_mut_slice().try_into().unwrap());

        // Perform a read operation
        let read_result = fil.io(
            IORequest::SyncRead,
            false,
            space_id,
            0,
            0,
            page_frame,
            IB_FILE_BLOCK_SIZE,
        );

        assert!(read_result.is_ok());
    }

    #[test]
    fn test_make_fil_name() {
        let os_io = Arc::new(Mutex::new(OsIo::new()));
        let fil = Fil::new(10, os_io);

        // Test with a valid file name
        let file_name = "test_file";
        let result = fil.make_ibd_filename(file_name);
        assert_eq!(result, "/tmp/test_file.ibd");
    }
}
