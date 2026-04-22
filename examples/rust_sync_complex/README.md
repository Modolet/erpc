# Rust sync complex example

This example demonstrates `erpcgen -g rust-sync` with a blocking Rust client and server.

The IDL covers enums, nested structs, lists, binary payloads, an `out` parameter, and a oneway method. The example uses `BlockingMemoryTransport` so it can run as one local process while still exercising the generated client/server bindings.

Regenerate bindings after changing the IDL:

```sh
Release/Linux/erpcgen/erpcgen -g rust-sync -o examples/rust_sync_complex/src examples/rust_sync_complex/telemetry.erpc
mv examples/rust_sync_complex/src/telemetry.rs examples/rust_sync_complex/src/generated.rs
```

Run:

```sh
nix develop /nixos#rust --command cargo run --manifest-path examples/rust_sync_complex/Cargo.toml --offline
```
