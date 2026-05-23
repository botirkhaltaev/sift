mod cli;
mod e2e;
mod engine;
mod filter;
mod ignore;
mod output;
mod paths;
mod pattern;
mod search;
mod support;

use criterion::{criterion_group, criterion_main};

criterion_group!(
    name = cli;
    config = support::sift_criterion();
    targets =
        cli::bench,
        engine::bench,
        pattern::bench,
        output::bench,
        ignore::bench,
        filter::bench,
        paths::bench,
        search::bench,
        e2e::bench,
);
criterion_main!(cli);
