//! Build steps:
//! 1. Embeds the com0com driver files from vendor/com0com/ into the exe (with its ReadMe.txt,
//!    which carries com0com's GPL notice). Without them the exe still builds, and `install`
//!    looks for a com0com folder next to the exe.
//! 2. Windows only: embeds the app icon and version information in comtap.exe, so Explorer,
//!    Task Manager and the desktop shortcut show ComTap's icon, name and version.
//!    `FileVersion` and `ProductVersion` come from Cargo.toml.
use std::{env, fs, path::Path};

fn main() {
    embed_com0com();
    embed_windows_resources();
}

fn embed_com0com() {
    let dir = Path::new("vendor/com0com");
    println!("cargo:rerun-if-changed=vendor/com0com");
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    if let Ok(rd) = fs::read_dir(dir) {
        let mut names: Vec<_> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.eq_ignore_ascii_case("README.md"))
            .collect();
        names.sort();
        for n in names {
            let abs = fs::canonicalize(dir.join(&n)).unwrap();
            println!("cargo:rerun-if-changed={}", abs.display());
            out.push_str(&format!("    ({n:?}, include_bytes!({:?})),\n", abs.display().to_string()));
        }
    }
    out.push_str("];\n");
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("embedded.rs"), out).unwrap();
}

fn embed_windows_resources() {
    println!("cargo:rerun-if-changed=packaging/icons/comtap.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("packaging/icons/comtap.ico")
        .set("ProductName", "ComTap")
        .set("FileDescription", "ComTap: TCP devices as COM ports")
        .set("CompanyName", "Jack Brumley")
        .set("LegalCopyright", "Jack Brumley");
    // A failed resource compile fails the build: a Windows release must carry them.
    if let Err(error) = resource.compile() {
        panic!("Could not embed the Windows icon and version information: {error}");
    }
}
