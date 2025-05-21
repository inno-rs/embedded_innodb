# Embedded InnoDB

[![Rust](https://github.com/username/embedded_innodb/actions/workflows/rust.yml/badge.svg)](https://github.com/username/embedded_innodb/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/username/embedded_innodb/branch/main/graph/badge.svg)](https://codecov.io/gh/username/embedded_innodb)

A Rust implementation of the InnoDB storage engine.

## Code Coverage

This project uses [cargo-tarpaulin](https://github.com/xd009642/tarpaulin) for code coverage and [Codecov](https://codecov.io/) to display the results.
## Development

### Running Tests

```bash
cargo test
```

### Generating Code Coverage Locally

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --verbose
```