# forsinka

This is the source code for a webapp that can be used to check current delays in public transit in Norway.

On booting, the webapp will fetch all current public transit journeys, then, if configured, it will fetch updates every
`--fetch-interval-seconds`.

It can also serve a static json file that you can easily get with `curl`:

```shell
mkdir -p data &&
curl -H 'accept: application/json' -o data/example.json https://api.entur.io/realtime/v1/rest/et &&
cargo run --release serve -s data/example.json
```

## Getting started

This webapp is built with Rust, which you can get from [rustup](https://rustup.rs/). Or you can run `mise install`.

Compile with `cargo build`.

Run tests with `cargo test`.

Run the app with `cargo run`.