/// Alias for the standard `assert` macro, used for assertions that should always be checked.
///
/// This is used for critical assertions that must be verified in all build configurations,
/// including release builds. Use this for conditions that, if false, would lead to data corruption
/// or other serious issues.
pub(crate) use assert as ut_a;

/// Alias for the standard `debug_assert` macro, used for assertions that should only be checked in debug builds.
///
/// This is used for assertions that are important for debugging but might impact performance
/// in release builds. These assertions are only checked in debug builds and are no-ops in release builds.
pub(crate) use debug_assert as ut_ad;

use ctor::ctor;
use std::sync::Once;

/// Macro for triggering a panic with a custom error message.
///
/// This macro is used to indicate unrecoverable error conditions in the code.
/// It always triggers a panic with the provided message and optional format arguments.
/// Unlike `ut_a` and `ut_ad`, this macro is explicitly used to terminate execution
/// when an invalid state is detected, regardless of build configuration.
///
/// # Examples
///
/// ```
/// ut_panic!("Invalid state detected");
/// ut_panic!("Invalid value: {}", value);
/// ```
#[macro_export]
macro_rules! ut_panic {
    ($msg:expr $(, $args:tt)* ) => {
        panic!($msg $(, $args)*);
    };
}

/// Macro for unwrapping an Option and panicking with a custom error message if None.
///
/// This macro is used to extract the value from an Option and panic with a specified
/// error message if the Option is None. It's similar to the standard `unwrap()` method
/// but allows for custom error messages with format arguments.
///
/// # Examples
///
/// ```
/// let opt = Some(42);
/// let value = some_or_panic!(opt, "Expected a value");
/// assert_eq!(value, 42);
///
/// // With formatting:
/// let opt: Option<i32> = None;
/// // This would panic with the message: "Missing value for key: example"
/// // let value = some_or_panic!(opt, "Missing value for key: {}", "example");
/// ```
#[macro_export]
macro_rules! some_or_panic {
    ($opt:expr, $msg:expr $(, $args:tt)* ) => {
        match $opt {
            Some(val) => val,
            None => panic!($msg $(, $args)*),
        }
    };
}

/// Static guard to ensure tracing is initialized only once.
///
/// This uses the `Once` type from the standard library to ensure that the tracing
/// initialization code is executed exactly once, even if `init_tracing` is called
/// multiple times from different parts of the code.
static INIT: Once = Once::new();

/// Initializes the tracing system for logging and diagnostics.
///
/// This function sets up the tracing subscriber with a test writer and TRACE level logging.
/// It uses the `Once` guard to ensure that initialization happens only once, regardless
/// of how many times this function is called.
///
/// # Examples
///
/// ```
/// // Initialize tracing at the start of your program or tests
/// init_tracing();
///
/// // Now you can use tracing macros
/// tracing::info!("Application started");
/// ```
#[ctor]
fn init_tracing() {
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::TRACE)
            .init();
    });
}
