# Lustro API

## Clone

```bash
git clone https://github.com/Ligatum/Lustro_API.git
cd Lustro_API
```

## Build

```bash
# Pure Rust
cargo build --release

# C/C++ FFI bindings
cargo build --release --features ffi

# Python bindings
cargo build --release --features python

# FFI and Python bindings
cargo build --release --features ffi,python
```

## Architecture & API Reference

For detailed technical specifications, FFI type mappings, snapshot mechanics, and binding usage examples across Rust, C/C++, and Python, see **[ARCHITECTURE.md](ARCHITECTURE.md)**.
