use std::process::Command;

fn restore_client_clibs_libman() {
    // execute libman restore in src/bin/laser-precision-adjust-server
    let output = Command::new("libman")
        .arg("restore")
        .current_dir(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/bin/laser-precision-adjust-server"
        ))
        .output()
        .expect("Failed to execute 'libman restore'");

    if !output.status.success() {
        println!(
            "cargo:warning=libman restore failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("cargo:rerun-if-changed=src/bin/laser-precision-adjust-server/libman.json")
}

fn main() {
    restore_client_clibs_libman()
}
