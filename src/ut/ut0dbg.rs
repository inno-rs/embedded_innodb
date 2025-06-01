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
