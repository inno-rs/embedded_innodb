#[cfg(test)]
mod tests {
    use crate::innodb::innodb0types::{IB_FILE_BLOCK_SIZE, UNIV_PAGE_SIZE};
    use crate::os::os0file::{
        CreateMode, WriteResult, os_file_create, os_file_delete,
        os_file_get_size, os_file_read, os_file_set_size, os_file_write,
    };

    #[test]
    fn test_os_file_write() {
        // Create a test file
        let file_creation_result =
            os_file_create("test_file.txt", CreateMode::OsFileOpen);
        assert!(file_creation_result.is_some());

        let mut os_file = file_creation_result.unwrap();

        let mut data = b"Hello, world!".to_vec();
        data.resize(IB_FILE_BLOCK_SIZE, 0);
        let offset = 0;
        let len = data.len();

        // Write data to the file
        let result: Result<WriteResult, String> =
            os_file_write(&mut os_file, data.as_slice(), offset, len);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), WriteResult {});

        // Read the file to verify the write.
        let mut read_buffer = vec![0; IB_FILE_BLOCK_SIZE];
        let read_result = os_file_read(
            &mut os_file,
            offset,
            len,
            read_buffer.as_mut_slice(),
        );
        if let Err(e) = read_result {
            panic!("Failed to read file after write: {}", e);
        }
        assert!(read_result.is_ok());

        assert_eq!(read_buffer, data);
    }

    #[test]
    fn test_os_file_get_size() {
        // Create a test file
        let file_creation_result =
            os_file_create("test.txt", CreateMode::OsFileOpen);
        assert!(file_creation_result.is_some());

        let mut os_file = file_creation_result.unwrap();

        let mut data = b"Hello, world!".to_vec();
        data.resize(IB_FILE_BLOCK_SIZE, 0);
        let offset = 0;
        let len = data.len();

        // Write data to the file
        let result: Result<WriteResult, String> =
            os_file_write(&mut os_file, data.as_slice(), offset, len);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), WriteResult {});

        // Get the size of the file
        let size = os_file_get_size(&os_file);
        assert!(size >= 0, "File size should be non-negative");
        assert_eq!(
            size as usize,
            data.len(),
            "File size should match written data length"
        );
    }

    #[test]
    fn test_os_file_set_size() {
        let file_name = format!("test_{}", uuid::Uuid::new_v4()).to_string();
        let file_creation_result =
            os_file_create(&file_name, CreateMode::OsFileOpen);
        assert!(file_creation_result.is_some());

        let mut os_file = file_creation_result.unwrap();

        assert_eq!(os_file_get_size(&os_file), 0);

        let new_size = (UNIV_PAGE_SIZE * 10) as i64;

        let result = os_file_set_size(&mut os_file, new_size as u64);
        assert!(result.is_ok());
        assert_eq!(
            os_file_get_size(&os_file),
            new_size,
            "File size should match set size"
        );

        os_file_delete(&file_name).expect("should remove file");
    }
}
