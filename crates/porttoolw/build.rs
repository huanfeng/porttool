use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/porttool.res");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // Rebuild after changing the ICO: windres -i porttool.rc -I . -O res -o assets/porttool.res
    let resource = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("assets/porttool.res");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        let object = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("porttool-icon.o");
        let status = Command::new("windres")
            .args(["-i", resource.to_str().unwrap(), "-O", "coff", "-o", object.to_str().unwrap()])
            .status()
            .expect("Windows GNU builds require windres");
        assert!(status.success(), "failed to convert Windows icon resource");
        println!("cargo:rustc-link-arg-bin=porttoolw={}", object.display());
    } else {
        println!("cargo:rustc-link-arg-bin=porttoolw={}", resource.display());
    }
}
