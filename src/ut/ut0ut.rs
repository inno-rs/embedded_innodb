use once_cell::sync::Lazy;
use rand::{Rng, SeedableRng, distr::Alphanumeric, rngs::StdRng};
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

/// Generates a random alphanumeric string of specified length using a deterministic seed.
///
/// This function creates a random string using the provided seed value, which ensures
/// that the same seed will always produce the same string. This is useful for testing
/// scenarios where reproducible random data is needed.
///
/// # Arguments
///
/// * `len` - The length of the random string to generate
/// * `seed` - A seed value for the random number generator
///
/// # Returns
///
/// A random alphanumeric string of the specified length
pub fn random_string_with_seed(len: usize, seed: u64) -> String {
    let rng = StdRng::seed_from_u64(seed);
    rng.sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

/// Checks if all elements in a slice are equal, according to a custom comparison function.
///
/// This function allows for comparing elements using a custom equality function rather than
/// requiring the elements to implement the `Eq` trait.
/// It returns true if all elements
/// are equal, according to the provided comparison function, or if the slice is empty.
///
/// # Arguments
///
/// * `items` - A slice of elements to compare
/// * `equal` - A function that takes two references to elements and returns true if they are considered equal
///
/// # Returns
///
/// * `true` if all elements are equal according to the comparison function or if the slice is empty
/// * `false` if any element is not equal to the first element
pub fn all_equal_by<T, F>(items: &[T], mut equal: F) -> bool
where
    F: FnMut(&T, &T) -> bool,
{
    match items.first() {
        Some(first) => items.iter().all(|x| equal(first, x)),
        None => true,
    }
}
