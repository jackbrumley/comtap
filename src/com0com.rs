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
pub const REQUIRED: &[&str] =
    &["setupc.exe", "setup.dll", "com0com.inf", "com0com.sys", "com0com.cat", "cncport.inf", "comport.inf"];

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
                    bail!("{} is already used by another device. Pick another COM number in the config.", link.com);
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
    serialport::available_ports()
        .map(|ports| ports.iter().any(|p| p.port_name.eq_ignore_ascii_case(com)))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "       CNCA0 PortName=COM5\r\n       CNCB0 PortName=-,HiddenMode=yes\r\n\
                        \r\n       CNCA1 PortName=COM6,EmuBR=yes\r\n       CNCB1 PortName=-\r\n\
                        some other line\r\n       CNCA3 PortName=COM9\r\n       CNCB3 PortName=MYPORT\r\n";

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
