fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_path = "../eventfold-db/proto/eventfold.proto";
    println!("cargo:rerun-if-changed={proto_path}");
    tonic_build::compile_protos(proto_path)?;
    Ok(())
}
