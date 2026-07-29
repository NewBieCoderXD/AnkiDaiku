use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct PackageJson {
  pub name: Option<String>,
  pub description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AnkiDaikuConfig {
  pub cards_dir: Option<String>,
  pub shared_css: Option<String>,
  pub media_dir: Option<String>,
}

pub fn parse_package_json(root: &Path) -> Option<PackageJson> {
  let path = root.join("package.json");
  let content = fs::read_to_string(path).ok()?;
  serde_json::from_str(&content).ok()
}

pub fn parse_config(root: &Path) -> Option<AnkiDaikuConfig> {
  for ext in ["json"] {
    let path = root.join(format!("ankidaiku.config.{}", ext));
    if let Ok(content) = fs::read_to_string(&path) {
      if let Ok(config) = serde_json::from_str(&content) {
        return Some(config);
      }
    }
  }
  None
}

pub fn parse_config_path(path: &Path) -> Option<AnkiDaikuConfig> {
  let content = fs::read_to_string(path).ok()?;
  serde_json::from_str(&content).ok()
}
