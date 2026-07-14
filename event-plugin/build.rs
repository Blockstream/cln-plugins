use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);

    tonic_prost_build::configure()
        .build_server(false)
        .build_client(true)
        .out_dir(&out_dir)
        .compile_protos(
            &["./proto/events/v1/event.proto"],
            &["./proto", "/usr/include"],
        )?;

    println!("cargo:rerun-if-changed=./proto/events/v1/event.proto");
    Ok(())
}
