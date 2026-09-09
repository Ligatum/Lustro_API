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

## Speed Test and Validaton

The repo contains **Lustro_API_ffi_python_validator.py** and **Lustro_API_ffi_base_speed_test.py**. Both scripts require lustro.dll in the same folder.
