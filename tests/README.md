# Integration tests

Run with `cargo nextest run --all-features`.

| File | Covers |
|------|--------|
| `load.rs` | multi-format loading, layered merging and error paths, based on [`fixtures/`](../fixtures) |
| `watch.rs` | hot reloading against the real file system: change, atomic rename, parse-failure fallback, debounce de-duplication, clean shutdown |
