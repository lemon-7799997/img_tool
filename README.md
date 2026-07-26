# img_tool

A tiny Rust CLI tool for batch image resizing, sprite-atlas packing and image cutting, powered by `image` + `rayon` multithreading.

## Features

- **resize** — batch resize images with aspect-ratio control (keep/center or stretch), multi-threaded
- **atlas** — pack images into a single sprite atlas with tight packing or uniform-grid layout
- **cut** — slice a single image into a grid of smaller segments

## Build

### macOS / Linux

```bash
cargo build --release
```

### Windows (native)

```cmd
cargo build --release
```

### Cross-compile macOS → Windows (zigbuild)

Requires [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```bash
# Install the target & zigbuild (one-time)
rustup target add x86_64-pc-windows-gnullvm
cargo install cargo-zigbuild

# Build
cargo zigbuild --release --target x86_64-pc-windows-gnullvm
```

Output: `target/x86_64-pc-windows-gnullvm/release/img_tool.exe`

## Dependencies

- [`clap`](https://crates.io/crates/clap) — CLI argument parsing
- [`image`](https://crates.io/crates/image) — image decoding/encoding/resizing
- [`glob`](https://crates.io/crates/glob) — directory file scanning
- [`rayon`](https://crates.io/crates/rayon) — parallel processing
