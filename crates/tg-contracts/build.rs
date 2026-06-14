use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=proto/tg/v1");

    let proto_dir = PathBuf::from("proto/tg/v1");
    let mut protos = fs::read_dir(&proto_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
        .collect::<Vec<_>>();
    protos.sort();

    tonic_build::configure().compile(&protos, &[proto_dir])?;
    Ok(())
}
