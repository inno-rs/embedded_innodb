extern crate core;

use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

mod btr;
mod buf;
mod data;
mod dict;
mod fil;
mod fsp;
mod fut;
mod innodb;
mod lock;
mod log;
mod mach;
mod mtr;
mod os;
mod page;
mod rem;
mod row;
mod srv;
mod sync;
mod tests;
mod trx;
mod ut;
mod utils;

/// Initialize the tracing logger.
///
/// This function sets up the tracing subscriber with a default configuration.
/// The log level can be controlled via the RUST_LOG environment variable.
/// For example:
///   - RUST_LOG=info cargo run
///   - RUST_LOG=debug cargo run
///   - RUST_LOG=trace cargo run
///
/// If RUST_LOG is not set, the default level is INFO.
///
/// # Usage in code
///
/// After initialization, you can use macros from the [`tracing`] crate:
/// - [`info!`] for informational messages
/// - [`warn!`] for warnings
/// - [`error!`] for errors
/// - [`debug!`] for debug output (shown when the level is DEBUG or lower)
/// - [`trace!`] for detailed trace output (shown when the level is TRACE)
///
/// Example:
/// ```
/// use tracing::{info, warn, error};
///
/// info!("This is an informational message");
/// warn!("This is a warning message");
/// error!("This is an error message");
/// ```
fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    fmt::Subscriber::builder()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();
}

fn main() {
    // Initialize the tracing logger
    init_tracing();

    info!("Hello, world!");
}
