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
    /// The port your software opens: "COM5" on Windows, "ttyComTap0" (/dev/ttyComTap0) on Linux.
    pub com: String,
    /// Off: the device stays in the list and keeps its port, but ComTap doesn't connect to it,
    /// so the device's connection slot is free. Written only when off.
    #[serde(default = "on", skip_serializing_if = "is_on")]
    pub enabled: bool,
}

fn on() -> bool {
    true
}

fn is_on(enabled: &bool) -> bool {
    *enabled
}

#[cfg(windows)]
pub const EXAMPLE: &str = r#"{
  "links": [
    {
      "name": "Device 1",
      "tcp": "192.168.1.50:51236",
      "com": "COM5"
    }
  ]
}
"#;

#[cfg(not(windows))]
pub const EXAMPLE: &str = r#"{
  "links": [
    {
      "name": "Device 1",
      "tcp": "192.168.1.50:51236",
      "com": "ttyComTap0"
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
        cfg.validate()?;
        Ok(cfg)
    }

    /// Normalises port names and checks every link. An empty list is allowed.
    pub fn validate(&mut self) -> Result<()> {
        let cfg = self;
        for l in &mut cfg.links {
            l.com = crate::ports::normalize(&l.com)
                .map_err(|e| anyhow::anyhow!("link '{}': com {e}", l.name))?;
            l.tcp = l.tcp.trim().to_string();
            let ok = l
                .tcp
                .rsplit_once(':')
                .is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok_and(|p| p > 0));
            if !ok {
                bail!("link '{}': tcp must be host:port, got '{}'", l.name, l.tcp);
            }
        }
        for (i, a) in cfg.links.iter().enumerate() {
            if cfg.links[..i].iter().any(|b| b.com == a.com) {
                bail!("{} is used by more than one link", a.com);
            }
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)? + "\n")
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_parses() {
        let c = Config::parse(EXAMPLE).unwrap();
        assert_eq!(c.links.len(), 1);
    }

    #[test]
    fn enabled_defaults_on_and_is_written_only_when_off() {
        let com = if cfg!(windows) { "COM5" } else { "ttyComTap0" };
        let mut c = Config::parse(&format!(
            r#"{{"links":[{{"name":"a","tcp":"1.2.3.4:5","com":"{com}"}}]}}"#
        ))
        .unwrap();
        assert!(c.links[0].enabled);
        assert!(!serde_json::to_string(&c).unwrap().contains("enabled"));
        c.links[0].enabled = false;
        let text = serde_json::to_string(&c).unwrap();
        assert!(text.contains(r#""enabled":false"#), "{text}");
        assert!(!Config::parse(&text).unwrap().links[0].enabled);
    }

    #[cfg(windows)]
    #[test]
    fn normalises_and_rejects() {
        let c = Config::parse(r#"{"links":[{"name":"a","tcp":"1.2.3.4:5","com":" com7 "}]}"#);
        assert_eq!(c.unwrap().links[0].com, "COM7");
        assert!(
            Config::parse(r#"{"links":[{"name":"a","tcp":"1.2.3.4:5","com":"COMX"}]}"#).is_err()
        );
        assert!(Config::parse(r#"{"links":[{"name":"a","tcp":"nohost","com":"COM5"}]}"#).is_err());
        assert!(Config::parse(r#"{"links":[]}"#).unwrap().links.is_empty());
        assert!(Config::parse(r#"{"links":[{"name":"a","tcp":"h:99999","com":"COM5"}]}"#).is_err());
        assert!(
            Config::parse(
                r#"{"links":[{"name":"a","tcp":"h:1","com":"COM5"},{"name":"b","tcp":"h:2","com":"com5"}]}"#
            )
            .is_err()
        );
    }
}
