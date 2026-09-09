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

# Python
cargo build --release --features python

# FFI and Python
cargo build --release --features ffi,python
```

## Architecture & API Reference

For technical specifications, FFI mappings, snapshot mechanics, and binding examples across Rust, C/C++, and Python, see **[ARCHITECTURE.md](ARCHITECTURE.md)**.

---

Ps. Short usage examples for batch streaming and ease of use coming soon.
