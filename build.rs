fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vendored = "proto/eventfold.proto";
    let upstream = "../eventfold-db/proto/eventfold.proto";

    // When the upstream proto is available (local development), warn if the
    // vendored copy has drifted. This check is skipped on crates.io builds
    // where the sibling repo doesn't exist.
    if std::path::Path::new(upstream).exists() {
        println!("cargo:rerun-if-changed={upstream}");
        let v = std::fs::read(vendored).unwrap_or_default();
        let u = std::fs::read(upstream).unwrap_or_default();
        if v != u {
            println!(
                "cargo:warning=proto/eventfold.proto is out of sync with \
                 {upstream}. Run: cp {upstream} {vendored}"
            );
        }
    }

    println!("cargo:rerun-if-changed={vendored}");
    tonic_build::compile_protos(vendored)?;
    Ok(())
}
