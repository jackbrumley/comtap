use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub links: Vec<Link>,
}

/// One TCP device shown as one COM port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub name: String,
    /// host:port of the device. The device is the TCP server.
    pub tcp: String,
    /// The COM port your software opens, e.g. "COM5".
    pub com: String,
}

pub const EXAMPLE: &str = r#"{
  "links": [
    {
      "name": "Line 1 scanner",
      "tcp": "192.168.1.50:51236",
      "com": "COM5"
    }
  ]
}
"#;

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Config::parse(&text).with_context(|| format!("in config {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config> {
        let mut cfg: Config = serde_json::from_str(text)?;
        if cfg.links.is_empty() {
            bail!("no links: add at least one under \"links\"");
        }
        for l in &mut cfg.links {
            l.com = l.com.trim().to_ascii_uppercase();
            l.tcp = l.tcp.trim().to_string();
            let n = l.com.strip_prefix("COM").and_then(|n| n.parse::<u32>().ok());
            if !matches!(n, Some(1..=255)) {
                bail!("link '{}': com must look like COM5, got '{}'", l.name, l.com);
            }
            if !l.tcp.contains(':') {
                bail!("link '{}': tcp must be host:port, got '{}'", l.name, l.tcp);
            }
        }
        for (i, a) in cfg.links.iter().enumerate() {
            if cfg.links[..i].iter().any(|b| b.com == a.com) {
                bail!("{} is used by more than one link", a.com);
            }
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_parses() {
        let c = Config::parse(EXAMPLE).unwrap();
        assert_eq!(c.links[0].com, "COM5");
    }

    #[test]
    fn normalises_and_rejects() {
        let c = Config::parse(r#"{"links":[{"name":"a","tcp":"1.2.3.4:5","com":" com7 "}]}"#);
        assert_eq!(c.unwrap().links[0].com, "COM7");
        assert!(Config::parse(r#"{"links":[{"name":"a","tcp":"1.2.3.4:5","com":"COMX"}]}"#).is_err());
        assert!(Config::parse(r#"{"links":[{"name":"a","tcp":"nohost","com":"COM5"}]}"#).is_err());
        assert!(Config::parse(r#"{"links":[]}"#).is_err());
        assert!(
            Config::parse(
                r#"{"links":[{"name":"a","tcp":"h:1","com":"COM5"},{"name":"b","tcp":"h:2","com":"com5"}]}"#
            )
            .is_err()
        );
    }
}
