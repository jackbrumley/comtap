//! Embeds the com0com driver files from vendor/com0com/ into the exe, if they are there.
//! Without them the exe still builds, and `install` looks for a com0com folder next to the exe.
use std::{env, fs, path::Path};

fn main() {
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
