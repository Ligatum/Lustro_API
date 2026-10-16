fn main() {
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_FFI");
    println!("cargo:rerun-if-env-changed=LUSTRO_HEADER_DIR");

    if std::env::var_os("CARGO_FEATURE_FFI").is_some() {
        generate_header();
    }
}

fn generate_header() {
    use cbindgen::{EnumConfig, RenameRule};
    use std::path::PathBuf;

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let mut config = cbindgen::Config::default();
    config.language = cbindgen::Language::C;
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
         typedef enum {\n\
         \tLUSTRO_ERROR_OK                  = 0,\n\
         \tLUSTRO_ERROR_INVALID_LENGTH      = 1,\n\
         \tLUSTRO_ERROR_INVALID_POINTER     = 2,\n\
         \t/* 3..5: part of the ABI, not returned by V1 functions */\n\
         \tLUSTRO_ERROR_OUTPUT_TOO_SMALL    = 3,\n\
         \tLUSTRO_ERROR_ALREADY_FINALISED   = 4,\n\
         \tLUSTRO_ERROR_VERIFICATION_FAILED = 5,\n\
         \tLUSTRO_ERROR_INTERNAL_PANIC      = 6,\n\
         \t/* 7..10: reported by the constructors */\n\
         \tLUSTRO_ERROR_UNSUPPORTED_VERSION  = 7,\n\
         \tLUSTRO_ERROR_INVALID_KIND         = 8,\n\
         \tLUSTRO_ERROR_INVALID_CURSOR       = 9,\n\
         \tLUSTRO_ERROR_OUT_OF_MEMORY        = 10\n\
         } LustroError;"
            .to_string(),
    );
    config.trailer = Some(
        "#ifdef __cplusplus\n\
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
