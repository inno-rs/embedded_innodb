#[cfg(test)]
mod tests {
    use crate::fil::fil0fil::Fil;
    use crate::fil::fil0types::FilType;

    #[test]
    fn test_create_fil_space() {
        let mut fil = Fil::new(10);

        let result = fil.space_create(
            "test".to_string(),
            10,
            0,
            FilType::FIL_TABLESPACE,
        );

        // FilSpace is empty. Creating a new one should return true.
        assert!(result);

        // Adding the same name again should fail.
        {
            let result = fil.space_create(
                "test".to_string(),
                10,
                0,
                FilType::FIL_TABLESPACE,
            );

            assert!(!result);
        }

        // Adding a different name but the same space_id should fail as well
        {
            let result = fil.space_create(
                "test_2".to_string(),
                10,
                0,
                FilType::FIL_TABLESPACE,
            );

            assert!(!result);
        }

        // Adding a totally different name and space_id should succeed.

        {
            let result = fil.space_create(
                "test_2".to_string(),
                11,
                0,
                FilType::FIL_TABLESPACE,
            );

            assert!(result);
        }
    }

    #[test]
    fn test_node_create() {
        let mut fil = Fil::new(10);

        // No space is present, the result should be false.
        let file_name = "test";
        let result = fil.node_create(file_name, 100, 20, false);
        assert!(!result);

        let fil_space_result = fil.space_create(
            file_name.to_string(),
            20,
            20,
            FilType::FIL_TABLESPACE,
        );
        assert!(fil_space_result);

        // Retry creating fil_node
        let result = fil.node_create(file_name, 100, 20, false);
        assert!(result);
    }

    #[test]
    fn test_space_free() {
        let mut fil = Fil::new(10);

        // No space is present, the result should be false.
        let file_name = "test";

        let fil_space_result = fil.space_create(
            file_name.to_string(),
            20,
            20,
            FilType::FIL_TABLESPACE,
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
}
