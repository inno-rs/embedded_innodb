#[cfg(test)]
mod tests {
    use crate::fil::fil0fil::Fil;
    use crate::fil::fil0types::{FilType, SpaceId};

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
}
