pub(crate) use assert as ut_a;
pub(crate) use debug_assert as ut_ad;

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
        assert!(false, $msg $(, $args)*);
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
