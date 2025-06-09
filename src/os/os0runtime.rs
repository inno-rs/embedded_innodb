use std::sync::Arc;

use tokio::runtime::Runtime;

/// Creates a new Tokio runtime with the specified number of worker threads.
///
/// # Arguments
/// * `number_of_threads` - The number of worker threads to use in the runtime.
///
/// # Returns
/// An `Arc<Runtime>` that can be shared across the application.
pub fn create_runtime(number_of_threads: usize) -> Arc<Runtime> {
    Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(number_of_threads)
            .thread_name("os0runtime-worker")
            .enable_all()
            .build()
            .unwrap(),
    )
}
