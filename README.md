# Embedded InnoDB

[![Rust](https://github.com/username/embedded_innodb/actions/workflows/rust.yml/badge.svg)](https://github.com/username/embedded_innodb/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/username/embedded_innodb/branch/main/graph/badge.svg)](https://codecov.io/gh/username/embedded_innodb)

A Rust implementation of the InnoDB storage engine, designed to be embedded in other applications.
This project aims to provide a reliable, high-performance storage engine with ACID compliance,
similar to the original InnoDB but with the safety and concurrency benefits of Rust.

## Project Status

This project is currently in early development. The following components are being actively
developed:

- **Buffer Pool Management** (buf): Implementation of the buffer pool for caching pages in memory
- **B-tree Operations** (btr): Implementation of B-tree data structures for indexing
- **Synchronization Primitives** (sync): Thread synchronization mechanisms
- **File Operations** (fil): File I/O operations
- **Page Management** (page): Page structure and operations
- **Transaction Management** (trx): ACID transaction support
- **Logging** (log): Redo and undo logging for crash recovery

## Features

- ACID compliant transactions
- B-tree based indexing
- Buffer pool for caching
- Multi-version concurrency control (MVCC)
- Crash recovery through redo logging
- Written in safe Rust with minimal unsafe code

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
embedded_innodb = "0.1.0"
```

## Usage

```rust
use embedded_innodb::innodb;

fn main() {
    // Initialize the database
    // Note: API is still under development

    // Example usage will be provided as the API stabilizes
}
```

## Code Coverage

This project uses [cargo-tarpaulin](https://github.com/xd009642/tarpaulin) for code coverage
and [Codecov](https://codecov.io/) to display the results.

## Development

### Requirements

- Rust 2024 edition or later
- Cargo

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Generating Code Coverage Locally

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --verbose
```

### Logging

The project uses the `tracing` crate for logging. You can control the log level using the `RUST_LOG`
environment variable:

```bash
RUST_LOG=info cargo run
RUST_LOG=debug cargo run
RUST_LOG=trace cargo run
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Please contact the repository owner for license information.
