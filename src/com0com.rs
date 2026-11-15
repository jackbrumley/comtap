//! Drives the com0com driver through its command-line tool, setupc.exe.
//!
//! Each link gets a pair: side A is named after the link (COM5) and is what your software opens.
//! Side B keeps com0com's default name (CNCB<n>), is hidden from other apps, and is what ComTap opens.
use crate::config::Link;
use crate::log;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

/// Files setupc needs beside it to create pairs and install the driver.
/// (com0com 2.2.2.0 signed x64: the .sys is signed through com0com.cat.)
pub const REQUIRED: &[&str] = &["setupc.exe", "setup.dll", "com0com.inf", "com0com.sys", "com0com.cat"];

pub fn has_embedded() -> bool {
    !embedded::FILES.is_empty()
}

fn missing_in(dir: &Path) -> Vec<&'static str> {
    REQUIRED.iter().copied().filter(|f| !dir.join(f).is_file()).collect()
}

/// Puts the com0com files into `dest`: from inside the exe if built with them, else from a
/// com0com folder next to the exe.
pub fn deploy_files(dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    if has_embedded() {
        for (name, bytes) in embedded::FILES {
            let p = dest.join(name);
            let same = std::fs::read(&p).map(|b| b == *bytes).unwrap_or(false);
            if !same {
                std::fs::write(&p, bytes).with_context(|| format!("writing {}", p.display()))?;
            }
        }
    } else {
        let src = crate::paths::exe_dir().join("com0com");
        if !missing_in(&src).is_empty() {
            bail!(
                "this comtap.exe was built without the com0com files, and there is no complete com0com folder \
                 next to it ({}). See vendor/com0com/README.md.",
                src.display()
            );
        }
        if src != dest {
            for e in std::fs::read_dir(&src)?.flatten() {
                if e.path().is_file() {
                    std::fs::copy(e.path(), dest.join(e.file_name()))?;
                }
            }
        }
    }
    let missing = missing_in(dest);
    if !missing.is_empty() {
        bail!("com0com files missing from {}: {}", dest.display(), missing.join(", "));
    }
    Ok(())
}

/// Where setupc.exe is: the installed copy, else a com0com folder next to the exe.
pub fn locate() -> Result<PathBuf> {
    for dir in [crate::paths::installed_com0com(), crate::paths::exe_dir().join("com0com")] {
        if missing_in(&dir).is_empty() {
            return Ok(dir);
        }
    }
    bail!("com0com is not set up yet. Run `comtap install` as administrator.")
}

/// Adds the com0com driver's signer to the machine's Trusted Publishers, so the first driver
/// install doesn't stop at a "Would you like to install this device software?" prompt.
pub fn trust_publisher(dir: &Path) -> Result<()> {
    let cat = dir.join("com0com.cat");
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $c=(Get-AuthenticodeSignature -LiteralPath '{}').SignerCertificate; \
         if(-not $c){{ throw 'com0com.cat is not signed' }}; \
         $s=New-Object System.Security.Cryptography.X509Certificates.X509Store('TrustedPublisher','LocalMachine'); \
         $s.Open('ReadWrite'); $s.Add($c); $s.Close(); $c.Subject",
        cat.display().to_string().replace('\'', "''")
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .context("running powershell")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    log!("Trusted driver publisher: {}", String::from_utf8_lossy(&out.stdout).trim());
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub index: u32,
    pub a: Vec<(String, String)>,
    pub b: Vec<(String, String)>,
}

fn param<'a>(p: &'a [(String, String)], key: &str) -> Option<&'a str> {
    p.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

impl Pair {
    pub fn a_name(&self) -> String {
        match param(&self.a, "PortName") {
            Some(n) if n != "-" => n.to_string(),
            _ => format!("CNCA{}", self.index),
        }
    }

    /// The path ComTap opens for side B.
    pub fn b_device(&self) -> String {
        match param(&self.b, "PortName") {
            Some(n) if n != "-" => format!(r"\\.\{n}"),
            _ => format!(r"\\.\CNCB{}", self.index),
        }
    }

    pub fn b_hidden(&self) -> bool {
        param(&self.b, "HiddenMode").is_some_and(|v| v.eq_ignore_ascii_case("yes"))
    }
}

/// Parses `setupc list` output, e.g.
/// ```text
///        CNCA0 PortName=COM5
///        CNCB0 PortName=-,HiddenMode=yes
/// ```
pub fn parse_list(text: &str) -> Vec<Pair> {
    let mut pairs: Vec<Pair> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let (id, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let id = id.to_ascii_uppercase();
        let Some(tail) = id.strip_prefix("CNC") else { continue };
        let mut chars = tail.chars();
        let side = chars.next();
        let Ok(index) = chars.as_str().parse::<u32>() else { continue };
        let params: Vec<(String, String)> = rest
            .trim()
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        let pos = match pairs.iter().position(|p| p.index == index) {
            Some(i) => i,
            None => {
                pairs.push(Pair { index, a: vec![], b: vec![] });
                pairs.len() - 1
            }
        };
        match side {
            Some('A') => pairs[pos].a = params,
            Some('B') => pairs[pos].b = params,
            _ => {}
        }
    }
    pairs.sort_by_key(|p| p.index);
    pairs
}

pub struct Setupc {
    dir: PathBuf,
}

impl Setupc {
    pub fn new(dir: PathBuf) -> Setupc {
        Setupc { dir }
    }

    pub fn find() -> Result<Setupc> {
        Ok(Setupc::new(locate()?))
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        // setupc finds setup.dll and the .inf files in its working directory.
        let out = Command::new(self.dir.join("setupc.exe"))
            .current_dir(&self.dir)
            .arg("--silent")
            .args(args)
            .output()
            .with_context(|| format!("running setupc in {}", self.dir.display()))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if !out.status.success() {
            bail!("setupc {} failed ({}): {}", args.join(" "), out.status, text.trim());
        }
        Ok(text)
    }

    pub fn list(&self) -> Result<Vec<Pair>> {
        Ok(parse_list(&self.run(&["list"])?))
    }

    pub fn install_pair(&self, com: &str) -> Result<()> {
        let a = format!("PortName={com}");
        self.run(&["install", &a, "PortName=-,HiddenMode=yes"])?;
        Ok(())
    }

    pub fn change(&self, id: &str, params: &str) -> Result<()> {
        self.run(&["change", id, params])?;
        Ok(())
    }

    pub fn remove(&self, index: u32) -> Result<()> {
        self.run(&["remove", &index.to_string()])?;
        Ok(())
    }

    /// Removes every pair and the driver itself.
    pub fn uninstall_driver(&self) -> Result<()> {
        self.run(&["uninstall"])?;
        Ok(())
    }
}

/// Makes sure each link has its pair, creating or fixing pairs as needed.
/// Returns the device path ComTap should open for each link, in config order.
pub fn reconcile(setupc: &Setupc, links: &[Link]) -> Result<Vec<String>> {
    let mut pairs = setupc.list()?;
    let mut out = Vec::new();
    for link in links {
        let pair = match pairs.iter().find(|p| p.a_name().eq_ignore_ascii_case(&link.com)) {
            Some(p) => p.clone(),
            None => {
                if let Some(p) = pairs.iter().find(|p| {
                    param(&p.b, "PortName").is_some_and(|n| n.eq_ignore_ascii_case(&link.com))
                }) {
                    bail!(
                        "{} is the B side of com0com pair {} (not made by ComTap). Remove it in the com0com setup, \
                         or pick another COM number.",
                        link.com,
                        p.index
                    );
                }
                if com_in_use(&link.com) {
                    bail!(
                        "{} is reserved by another device (one that is plugged in, or was before). \
                         Pick a free COM port for '{}'.",
                        link.com,
                        link.name
                    );
                }
                log!("Creating virtual port pair for {} ({})", link.com, link.name);
                setupc.install_pair(&link.com)?;
                pairs = setupc.list()?;
                pairs
                    .iter()
                    .find(|p| p.a_name().eq_ignore_ascii_case(&link.com))
                    .cloned()
                    .with_context(|| format!("setupc ran, but no pair named {} appeared", link.com))?
            }
        };
        if !pair.b_hidden() {
            let id = format!("CNCB{}", pair.index);
            match setupc.change(&id, "HiddenMode=yes") {
                Ok(()) => log!("Hid {id} (ComTap's side of {})", link.com),
                Err(e) => log!("Warning: could not hide {id}: {e:#}"),
            }
        }
        log!("{} <-> {} ready for {}", link.com, pair.b_device(), link.name);
        out.push(pair.b_device());
    }
    Ok(out)
}

/// Removes the pairs whose A side is one of these links' COM ports.
pub fn remove_pairs(setupc: &Setupc, links: &[Link]) -> Result<()> {
    for p in setupc.list()? {
        if links.iter().any(|l| p.a_name().eq_ignore_ascii_case(&l.com)) {
            setupc.remove(p.index)?;
            log!("Removed virtual port pair {} ({})", p.index, p.a_name());
        }
    }
    Ok(())
}

fn com_in_use(com: &str) -> bool {
    let n = com.get(3..).and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
    comdb_reserved().contains(&n)
        || serialport::available_ports()
            .map(|ports| ports.iter().any(|p| p.port_name.eq_ignore_ascii_case(com)))
            .unwrap_or(false)
}

/// COM numbers Windows has reserved in its COM port database (ComDB). A number stays reserved
/// after its device (e.g. a USB serial adapter) is unplugged, and setupc stops with a dialog
/// if asked to use one, so ComTap only offers numbers that aren't in here.
pub fn comdb_reserved() -> Vec<u32> {
    let out = Command::new("reg")
        .args(["query", r"HKLM\SYSTEM\CurrentControlSet\Control\COM Name Arbiter", "/v", "ComDB"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    parse_comdb(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_comdb(reg_output: &str) -> Vec<u32> {
    let Some(hex) = reg_output
        .lines()
        .find(|l| l.contains("REG_BINARY"))
        .and_then(|l| l.split_whitespace().last())
    else {
        return Vec::new();
    };
    let bytes: Vec<u8> =
        (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()).collect();
    let mut out = Vec::new();
    for (i, b) in bytes.iter().enumerate() {
        for bit in 0..8 {
            if b & (1 << bit) != 0 {
                out.push((i * 8 + bit + 1) as u32);
            }
        }
    }
    out
}

/// COM ports a new link can use: not reserved in ComDB, not present, not in `taken`.
pub fn free_ports(taken: &[String], limit: usize) -> Vec<String> {
    let reserved = comdb_reserved();
    let present: Vec<String> = serialport::available_ports()
        .map(|v| v.into_iter().map(|p| p.port_name.to_ascii_uppercase()).collect())
        .unwrap_or_default();
    (1..=255u32)
        .filter(|n| !reserved.contains(n))
        .map(|n| format!("COM{n}"))
        .filter(|c| !present.contains(c) && !taken.contains(c))
        .take(limit)
        .collect()
}

/// com0com driver packages in the driver store that aren't 2.2.2.0, as (published name, version).
/// Windows prefers the newest version for new pairs, and com0com 3.0 is unsigned for x64,
/// so a leftover 3.0 package makes every new pair fail with code 52.
pub fn other_driver_packages() -> Vec<(String, String)> {
    let Ok(out) = Command::new("pnputil").arg("/enum-drivers").output() else { return Vec::new() };
    parse_pnputil(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_pnputil(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let field = |key: &str| {
            block
                .lines()
                .find(|l| l.trim_start().starts_with(key))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        };
        if !field("Original Name").is_some_and(|n| n.eq_ignore_ascii_case("com0com.inf")) {
            continue;
        }
        let (Some(oem), Some(ver)) = (field("Published Name"), field("Driver Version")) else { continue };
        let ver = ver.split_whitespace().last().unwrap_or("").to_string();
        if ver != "2.2.2.0" {
            found.push((oem, ver));
        }
    }
    found
}

pub fn delete_driver_package(oem: &str) -> Result<()> {
    let out = Command::new("pnputil").args(["/delete-driver", oem, "/uninstall", "/force"]).output()?;
    if !out.status.success() {
        bail!("pnputil /delete-driver {oem}: {}", String::from_utf8_lossy(&out.stdout).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "       CNCA0 PortName=COM5\r\n       CNCB0 PortName=-,HiddenMode=yes\r\n\
                        \r\n       CNCA1 PortName=COM6,EmuBR=yes\r\n       CNCB1 PortName=-\r\n\
                        some other line\r\n       CNCA3 PortName=COM9\r\n       CNCB3 PortName=MYPORT\r\n";

    #[test]
    fn parses_comdb_bitmap() {
        // This PC's real value: COM1, COM3-COM22 reserved.
        let out = "\r\nHKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\COM Name Arbiter\r\n    ComDB    REG_BINARY    FDFF3F0000000000\r\n";
        let r = parse_comdb(out);
        assert_eq!(r[0], 1);
        assert!(!r.contains(&2));
        assert!(r.contains(&3) && r.contains(&22));
        assert!(!r.contains(&23));
        assert_eq!(r.len(), 21);
    }

    #[test]
    fn finds_other_com0com_versions() {
        let out = "Microsoft PnP Utility\r\n\r\nPublished Name:     oem12.inf\r\nOriginal Name:      usbser.inf\r\nDriver Version:     01/01/2020 1.0.0.0\r\n\r\n\
                   Published Name:     oem362.inf\r\nOriginal Name:      com0com.inf\r\nProvider Name:      Vyacheslav Frolov\r\nDriver Version:     06/04/2012 3.0.0.0\r\n\r\n\
                   Published Name:     oem400.inf\r\nOriginal Name:      com0com.inf\r\nDriver Version:     01/25/2011 2.2.2.0\r\n";
        assert_eq!(parse_pnputil(out), vec![("oem362.inf".to_string(), "3.0.0.0".to_string())]);
    }

    #[test]
    fn parses_setupc_list() {
        let p = parse_list(LIST);
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].a_name(), "COM5");
        assert_eq!(p[0].b_device(), r"\\.\CNCB0");
        assert!(p[0].b_hidden());
        assert_eq!(p[1].a_name(), "COM6");
        assert!(!p[1].b_hidden());
        assert_eq!(p[2].index, 3);
        assert_eq!(p[2].b_device(), r"\\.\MYPORT");
    }

    #[test]
    fn unnamed_a_side_uses_default() {
        let p = parse_list("CNCA2 PortName=-\nCNCB2 PortName=-\n");
        assert_eq!(p[0].a_name(), "CNCA2");
    }
}
