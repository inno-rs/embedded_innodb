use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use innodb::ut::ut0rnd::ut_fold_string;
use rand::Rng;
use rand::distr::Alphanumeric;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

/// Generate 100,000 random strings of given length.
/// The strings are generated deterministically. So they are always the same.
fn generate_strings(count: usize, len: usize) -> Vec<String> {
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    (0..count)
        .map(|_| {
            (&mut rng)
                .sample_iter(&Alphanumeric)
                .take(len)
                .map(char::from)
                .collect()
        })
        .collect()
}

fn fold_buffer(c: &mut Criterion) {
    let input_strings = generate_strings(100_000, 100);

    c.bench_function("fold_buffer_bench", |b| {
        b.iter(|| {
            for input in input_strings.iter() {
                let fold = ut_fold_string(input.as_bytes());
                black_box(input);
            }
        })
    });
}

criterion_group!(benches, fold_buffer);
criterion_main!(benches);
