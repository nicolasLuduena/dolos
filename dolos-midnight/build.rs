fn main() {
    // Only generate subxt runtime bindings when the subxt-sync feature is enabled.
    // Without the feature, the parser falls back to MockParser.
    #[cfg(feature = "subxt-sync")]
    {
        use std::{env, fs::OpenOptions, io::Write, path::Path};

        let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
        let generated_path = Path::new(&out_dir).join("generated_runtime.rs");

        let metadata_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("metadata")
            .join("midnight_metadata.scale");
        let metadata_path = metadata_path
            .canonicalize()
            .unwrap_or_else(|_| panic!("metadata file not found at {}", metadata_path.display()));

        let generated_code = format!(
            r#"
            #[subxt::subxt(
                runtime_metadata_path = "{}",
                derive_for_type(
                    path = "sp_consensus_slots::Slot",
                    derive = "parity_scale_codec::Encode, parity_scale_codec::Decode",
                    recursive
                )
            )]
            pub mod midnight_runtime {{}}
            "#,
            metadata_path.display()
        );

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&generated_path)
            .unwrap_or_else(|e| {
                panic!(
                    "cannot open {} for writing: {e}",
                    generated_path.display()
                )
            });

        writeln!(file, "{generated_code}").expect("cannot write generated runtime code");

        println!("cargo:rerun-if-changed=metadata/midnight_metadata.scale");
    }
}
