# img_tool

A tiny Rust CLI tool for batch image resizing, sprite-atlas packing and image cutting, powered by `image` + `rayon` multithreading.

## Features

- **resize** — batch resize images with aspect-ratio control (keep/center or stretch), multi-threaded
- **atlas** — pack images into a single sprite atlas with tight packing or uniform-grid layout
- **cut** — slice a single image into a grid of smaller segments

## Usage

### `resize` — batch image resize

```bash
img_tool resize --input ./images --output ./out --size 256x256 --aspect keep
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--input` | `-i` | `.` | Input file or directory |
| `--output` | `-o` | *(required)* | Output directory |
| `--filter` | `-f` | `*.png` | Glob filter for directory scan |
| `--size` | `-s` | `256x256` | Target size `WxH` |
| `--aspect` | `-a` | `keep` | `keep` (center + transparent pad) or `stretch` |
| `--compression` | `-c` | `best` | PNG compression: `best`, `default`, `fast`, or `0`–`9` |

### `atlas` — sprite atlas packing

**Tight packing** (no `--resize`): preserves original dimensions, packs tightly row-by-row.

```bash
img_tool atlas -i ./images -o atlas.png
img_tool atlas -i ./images -o atlas.png --frames 4x3
img_tool atlas -i ./images -o atlas.png --atlas-size 4096x4096
```

**Uniform grid** (with `--resize`): all images resized to the same cell size first.

```bash
img_tool atlas -i ./images -o atlas.png --resize -s 256x256 --spacing 10x10
img_tool atlas -i ./images -o atlas.png --resize -s 128x128 --frames 8x8 --spacing 4x4
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--input` | `-i` | `.` | Input directory |
| `--output` | `-o` | *(required)* | Output atlas file path (e.g. `atlas.png`) |
| `--filter` | `-f` | `*.png` | Glob filter |
| `--resize` | `-r` | `false` | Enable per-image resize before packing |
| `--size` | `-s` | `256x256` | Cell size `WxH` (requires `--resize`) |
| `--aspect` | `-a` | `keep` | Aspect mode (requires `--resize`) |
| `--spacing` | | `0x0` | Gap between cells `WxH` (requires `--resize`) |
| `--frames` | | *(auto)* | Fixed grid `cols x rows` |
| `--atlas-size` | | *(auto)* | Force final atlas dimensions `WxH` |
| `--compression` | `-c` | `best` | PNG compression: `best`, `default`, `fast`, or `0`–`9` |

Auto-sizing: atlas dimensions are rounded up to the nearest multiple of 512, always square.

### `cut` — image cutting

Cut a single image into a grid of smaller segments.

```bash
img_tool cut -i input.png -o ./out --segments 4x4
img_tool cut -i input.png -o ./out --segments 3x2 -c default
img_tool cut -i input.png -o ./out -s 2x2 --name tile_$col-$row --index-start 1
```

| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--input` | `-i` | `.` | Input image file (must be a single file) |
| `--output` | `-o` | *(required)* | Output directory |
| `--segments` | `-s` | `4x4` | Grid size `cols x rows` |
| `--name` | | *(auto)* | Filename template with `$col`, `$row`, `$index` placeholders |
| `--index-start` | | `0` | Starting value for the `$index` placeholder |
| `--compression` | `-c` | `best` | PNG compression: `best`, `default`, `fast`, or `0`–`9` |

**Naming rules:** If `--name` contains at least one of `$col` / `$row` / `$index`, it is used as a template. Otherwise falls back to `{stem}-$index`. The `.png` extension is appended automatically. Characters illegal on Windows/macOS are rejected at startup.

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
