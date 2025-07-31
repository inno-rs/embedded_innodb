#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use rstest::*;

    use crate::{
        data::data0types::{DATA_INT, DTYPE_INT, DTuple, DType},
        dict::{
            dict0table::Table,
            dict0types::{Column, Field, Index, IndexType},
        },
        innodb::innodb0types::PageId,
        rem::rem0rec::OrphanRecord,
    };

    #[fixture]
    fn page_id() -> PageId {
        PageId { space_id: 0, page_no: 0 }
    }

    #[fixture]
    fn table() -> Arc<Table> {
        Arc::new(Table::new(100, "test_table".to_string()))
    }
    #[fixture]
    fn index(page_id: PageId, table: Arc<Table>) -> Arc<Index> {
        let mut index = Index::new(page_id);
        index.set_table(table);

        Arc::new(index)
    }
    #[fixture]
    fn column() -> Arc<Mutex<Column>> {
        Arc::new(Mutex::new(Column {
            name: "test_column".to_string(),
            index: 0,
            ord_part: 0,
            dtype: DTYPE_INT,
        }))
    }

    #[fixture]
    fn field(column: Arc<Mutex<Column>>) -> Field {
        Field {
            col: column,
            name: "test_field".to_string(),
            prefix_length: 0,
            fixed_length: 0,
        }
    }

    #[fixture]
    fn orphan_record() -> OrphanRecord {
        // Create an OrphanRecord with 1 field
        OrphanRecord::new(vec![0; 20], 1)
    }

    #[rstest]
    fn test_index_new(page_id: PageId) {
        let index = Index::new(page_id);

        // Verify the index has the expected properties
        assert_eq!(index.get_index_id(), 0);
        assert_eq!(index.get_page_id(), page_id);

        // Check default values
        assert_eq!(index.n_user_defined_cols(), 0);
        assert_eq!(index.n_uniq(), 0);
        assert_eq!(index.n_defined(), 0);
        assert_eq!(index.n_nullable(), 0);

        // Check the index type
        match index.index_type() {
            IndexType::DictClustered => {} // This is what we expect
            _ => panic!("Expected DictClustered, got a different index type"),
        }
    }

    #[rstest]
    fn test_get_nth_col(index: Arc<Index>, field: Field) {
        // Add a field to the index
        index.add_field(field);

        // Get the column and verify it's the same
        let col = index.get_nth_col(0);
        assert_eq!(col.lock().unwrap().name, "test_column");

        // Test out of bounds behavior
        let result = std::panic::catch_unwind(|| {
            index.get_nth_col(1); // This should panic
        });
        assert!(result.is_err());
    }

    #[rstest]
    fn test_get_index_id(index: Arc<Index>) {
        assert_eq!(index.get_index_id(), 0);
    }

    #[rstest]
    fn test_get_root_page_no(index: Arc<Index>, page_id: PageId) {
        assert_eq!(index.get_root_page_no(), page_id.page_no);
    }

    #[rstest]
    fn test_get_page_id(index: Arc<Index>, page_id: PageId) {
        assert_eq!(index.get_page_id(), page_id);
    }

    #[rstest]
    fn test_index_type(index: Arc<Index>) {
        match index.index_type() {
            IndexType::DictClustered => {} // This is what we expect
            _ => panic!("Expected DictClustered, got a different index type"),
        }
    }

    #[rstest]
    fn test_n_user_defined_cols(index: Arc<Index>) {
        assert_eq!(index.n_user_defined_cols(), 0);
    }

    #[rstest]
    fn test_n_uniq(index: Arc<Index>) {
        assert_eq!(index.n_uniq(), 0);

        // Test setting n_uniq
        index.set_n_uniq(5);
        assert_eq!(index.n_uniq(), 5);
    }

    #[rstest]
    fn test_set_n_uniq(index: Arc<Index>) {
        index.set_n_uniq(10);
        assert_eq!(index.n_uniq(), 10);
    }

    #[rstest]
    fn test_n_defined(index: Arc<Index>) {
        assert_eq!(index.n_defined(), 0);

        // Add a field and check if n_defined increases
        index.add_field(Field {
            col: Arc::new(Mutex::new(Column {
                name: "test_column".to_string(),
                index: 0,
                ord_part: 0,
                dtype: DTYPE_INT,
            })),
            name: "test_field".to_string(),
            prefix_length: 0,
            fixed_length: 0,
        });

        assert_eq!(index.n_defined(), 1);
    }

    #[rstest]
    #[ignore]
    fn test_n_nullable(index: Arc<Index>) {
        assert_eq!(index.n_nullable(), 0);

        // Add a nullable field and check if n_nullable increases
        // A field is nullable if it doesn't have DATA_NOT_NULL in its dtype.pr_type
        index.add_field(Field {
            col: Arc::new(Mutex::new(Column {
                name: "test_column".to_string(),
                index: 0,
                ord_part: 0,
                dtype: DType {
                    m_type: DATA_INT as u16,
                    pr_type: 0, // Not DATA_NOT_NULL
                    len: 4,
                    mb_min_len: 0,
                    mb_max_len: 0,
                },
            })),
            name: "test_field".to_string(),
            prefix_length: 0,
            fixed_length: 0,
        });

        assert_eq!(index.n_nullable(), 1);
    }

    #[rstest]
    fn test_page_id(index: Arc<Index>, page_id: PageId) {
        assert_eq!(index.page_id(), page_id);
    }

    #[rstest]
    fn test_add_field(index: Arc<Index>, field: Field) {
        // Add a field to the index
        index.add_field(field);

        // Verify the field was added
        assert_eq!(index.n_defined(), 1);
        let col = index.get_nth_col(0);
        assert_eq!(col.lock().unwrap().name, "test_column");
    }

    #[rstest]
    fn test_is_clustered(index: Arc<Index>) {
        // Default index type is DictClustered
        assert!(index.is_clustered());

        // We can't directly access the inner field, so we need to test the behavior
        // indirectly by checking the return value of is_clustered() and get_n_unique_in_tree()

        // For a clustered index, get_n_unique_in_tree() should return n_uniq
        index.set_n_uniq(5);
        assert_eq!(index.get_n_unique_in_tree(), 5);
    }

    #[rstest]
    fn test_get_n_unique_in_tree(index: Arc<Index>) {
        // For clustered index, get_n_unique_in_tree() should return n_uniq
        index.set_n_uniq(5);
        assert_eq!(index.get_n_unique_in_tree(), 5);
    }

    #[rstest]
    fn test_get_table(index: Arc<Index>) {
        // get_table() returns a dummy table
        let table = index.get_table();
        assert_eq!(table.get_id().0, 100);
        assert_eq!(table.get_name(), "test_table");
    }

    #[rstest]
    fn test_copy_types(index: Arc<Index>, field: Field) {
        // Add a field to the index
        index.add_field(field);

        // Create a DTuple and copy types
        let mut dtuple = DTuple::new(1);
        index.copy_types(&mut dtuple, 1);

        // Verify the types were copied
        assert_eq!(dtuple.n_fields, 1);
    }

    #[rstest]
    fn test_build_smallest_inner_node(index: Arc<Index>, field: Field) {
        // Add a field to the index
        index.add_field(field);
        index.set_n_uniq(1);

        // Build the smallest inner node
        let dtuple = index.build_smallest_inner_node(456);

        // Verify the inner node
        assert_eq!(dtuple.n_fields, 2);
    }

    #[rstest]
    fn test_check_search_tuple(index: Arc<Index>, field: Field) {
        // Add a field to the index
        index.add_field(field);
        index.set_n_uniq(1);

        // Create a DTuple with the correct number of fields
        let dtuple = DTuple::new(1);

        // Check the search tuple
        assert!(index.check_search_tuple(&dtuple));
    }

    #[rstest]
    #[should_panic]
    fn test_check_search_tuple_panics(index: Arc<Index>, field: Field) {
        // Add a field to the index
        index.add_field(field);

        // Create a DTuple with the correct number of fields
        let dtuple = DTuple::new(2);

        // Check the search tuple
        assert!(index.check_search_tuple(&dtuple));
    }
}
