# BYO/OS

A terminal-protocol-based desktop environment. All communication happens via ECMA-48 APC escape sequences over stdin/stdout.

## Requirements

- Rust 2024 edition (rustc 1.93+)

## Building

```sh
cargo check --workspace    # type-check all crates
cargo test --workspace     # run all tests
cargo build --workspace    # build all crates
```

## Structure

```
libs/byo              core protocol library (scanner, emitter, types)
system/compositor     Bevy/wgpu GPU renderer
system/orchestrator   process manager + APC router
services/controls     controls daemon (button, slider, checkbox)
```
