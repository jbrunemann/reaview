# Reaview

A fast, native macOS image viewer in the spirit of Preview.app, written in Rust.

- **← / →** step through every image in the same folder (Finder order)
- Opens anything macOS can decode: JPEG, PNG, HEIC, RAW, WebP, AVIF, GIF, TIFF, …
- Zoom to fit (⌘9), actual size (⌘0), zoom in/out (⌘+ / ⌘−), pinch to zoom
- Status bar with file type, pixel size, file size and zoom percentage
- Copy the image (⌘C) or its full path (⌥⌘C), also from the right-click menu

## Build

```sh
cargo run -- path/to/image.jpg   # run from source
./scripts/bundle.sh              # build target/Reaview.app
```

Requires macOS 11+ and a Rust toolchain.
