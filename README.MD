cargo build --release --features ffi,python<br>
cargo build --release --features ffi<br>
cargo build --release --features python<br>

# Lustro API Code

## Clone

```bash
git clone --filter=blob:none --sparse https://github.com/Ligatum/Lustro.git
cd Lustro
git sparse-checkout set API/LUSTRO_API_CODE
