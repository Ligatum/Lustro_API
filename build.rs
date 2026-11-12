fn main() {
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-env-changed=LUSTRO_HEADER_DIR");

    #[cfg(feature = "ffi")]
    generate_header();
}

#[cfg(feature = "ffi")]
fn generate_header() {
    use cbindgen::{EnumConfig, RenameRule};
    use std::path::PathBuf;

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let mut config = cbindgen::Config {
        language: cbindgen::Language::C,
        ..Default::default()
    };
    config.pragma_once = false;
    config.no_includes = true;

    config.enumeration = EnumConfig {
        rename_variants: RenameRule::ScreamingSnakeCase,
        prefix_with_name: true,
        ..Default::default()
    };

    // LustroError is emitted manually.
    // Keep this definition synchronized with src/errors.rs.
    config.export.exclude = vec!["LustroError".to_string()];

    config.header = Some(
        "#pragma once\n\
         #include <stdint.h>\n\
         #include <stddef.h>\n\
         #ifdef __cplusplus\n\
         extern \"C\" {\n\
         #endif\n\
         \n\
         /* Fixed width, as in Rust (`#[repr(i32)]`); a C enum can be narrower, e.g. -fshort-enums. */\n\
         typedef int32_t LustroError;\n\
         enum {\n\
         \tLUSTRO_ERROR_OK                  = 0,\n\
         \tLUSTRO_ERROR_INVALID_LENGTH      = 1,\n\
         \tLUSTRO_ERROR_INVALID_POINTER     = 2,\n\
         \t/* 3..5: part of the ABI, not returned by V1 functions */\n\
         \tLUSTRO_ERROR_OUTPUT_TOO_SMALL    = 3,\n\
         \tLUSTRO_ERROR_ALREADY_FINALISED   = 4,\n\
         \tLUSTRO_ERROR_VERIFICATION_FAILED = 5,\n\
         \tLUSTRO_ERROR_INTERNAL_PANIC      = 6,\n\
         \t/* 7..10: reported by the constructors (7 and 8 also by lustro_snapshot_kind) */\n\
         \tLUSTRO_ERROR_UNSUPPORTED_VERSION  = 7,\n\
         \tLUSTRO_ERROR_INVALID_KIND         = 8,\n\
         \tLUSTRO_ERROR_INVALID_CURSOR       = 9,\n\
         \tLUSTRO_ERROR_OUT_OF_MEMORY        = 10,\n\
         \t/* 11: lustro_configure; 12: lustro_configure, lustro_*_batch_lane and\n\
         \t * lustro_*_batch_from_streams; 13: the entropy functions */\n\
         \tLUSTRO_ERROR_ALREADY_CONFIGURED   = 11,\n\
         \tLUSTRO_ERROR_INVALID_ARGUMENT     = 12,\n\
         \tLUSTRO_ERROR_ENTROPY_UNAVAILABLE  = 13,\n\
         \t/* 14: snapshot import */\n\
         \tLUSTRO_ERROR_INVALID_RESERVED     = 14\n\
         };\n\
         \n\
         /*\n\
         \x20* 128-bit values.\n\
         \x20* Inputs (stream IDs) are passed as two uint64_t: `hi` (upper 64 bits) first, then `lo`.\n\
         \x20* Arrays of IDs are parallel arrays `*_hi` and `*_lo` of the same length.\n\
         \x20* Outputs (`lustro_*_next_u128`, `lustro_*_position`) are 16 bytes, little-endian:\n\
         \x20* byte 0 is the lowest byte of `lo`, byte 15 the highest byte of `hi`.\n\
         \x20* The two helpers below convert between the forms. They do not depend on the host\n\
         \x20* byte order and do not check their pointers.\n\
         \x20*/\n\
         static inline void lustro_u128_from_parts(uint64_t hi, uint64_t lo, uint8_t out[16])\n\
         {\n\
         \tfor (int i = 0; i < 8; i++) {\n\
         \t\tout[i] = (uint8_t)(lo >> (8 * i));\n\
         \t\tout[8 + i] = (uint8_t)(hi >> (8 * i));\n\
         \t}\n\
         }\n\
         \n\
         static inline void lustro_u128_to_parts(const uint8_t in[16], uint64_t *hi, uint64_t *lo)\n\
         {\n\
         \tuint64_t h = 0, l = 0;\n\
         \tfor (int i = 7; i >= 0; i--) {\n\
         \t\tl = (l << 8) | in[i];\n\
         \t\th = (h << 8) | in[8 + i];\n\
         \t}\n\
         \t*hi = h;\n\
         \t*lo = l;\n\
         }"
            .to_string(),
    );
    config.trailer = Some(
        "/**\n\
         \x20* Byte offset of the block for `step` and `lane` in the output of\n\
         \x20* `lustro_*_batch_fill_blocks`: `(step * batch_len + lane) * LUSTRO_BLOCK_LEN`.\n\
         \x20* Preconditions: `step < steps`, `lane < batch_len`, and\n\
         \x20* `steps * batch_len * LUSTRO_BLOCK_LEN` fits in `size_t`.\n\
         \x20* The arguments are not validated and overflow is not checked.\n\
         \x20*/\n\
         static inline size_t lustro_batch_block_offset(size_t step, size_t batch_len, size_t lane)\n\
         {\n\
         \treturn (step * batch_len + lane) * LUSTRO_BLOCK_LEN;\n\
         }\n\
         \n\
         #ifdef __cplusplus\n\
         }\n\
         #endif"
            .to_string(),
    );

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(out_dir.join("lustro.h"));

    // The source tree is touched only on request: LUSTRO_HEADER_DIR=include
    if let Some(dir) = std::env::var_os("LUSTRO_HEADER_DIR") {
        let dir = PathBuf::from(&crate_dir).join(dir);
        std::fs::create_dir_all(&dir).expect("cannot create LUSTRO_HEADER_DIR");
        let generated = std::fs::read(out_dir.join("lustro.h")).unwrap();
        let target = dir.join("lustro.h");
        println!("cargo:rerun-if-changed={}", target.display());
        if std::fs::read(&target).ok().as_deref() != Some(generated.as_slice()) {
            std::fs::write(&target, generated).expect("cannot write lustro.h");
        }
    }
}
