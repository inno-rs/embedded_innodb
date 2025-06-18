use once_cell::sync::Lazy;
use tokio::runtime::Runtime;

/// A macro that executes an asynchronous expression on the global runtime.
///
/// This macro provides a convenient way to execute asynchronous code in a synchronous context
/// by using the global Tokio runtime.
/// It is particularly useful for code that needs to perform
/// asynchronous operations but is called from synchronous code.
///
#[macro_export]
macro_rules! block_on {
    ($expr:expr) => {
        $crate::GLOBAL_RUNTIME.block_on($expr)
    };
}

/// A global Tokio runtime that can be used throughout the application.
///
/// This static is lazily initialized the first time it's accessed, creating a new Tokio runtime
/// with default configuration.
/// It's used by the `block_on!` macro to execute asynchronous code
/// in a synchronous context.
///
/// # Panics
///
/// The initialization will panic if the Tokio runtime cannot be created.
pub static GLOBAL_RUNTIME: Lazy<Runtime> =
    Lazy::new(|| Runtime::new().expect("Failed to create Tokio runtime"));
