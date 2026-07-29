use std::collections::HashSet;
use std::fs;
use std::path::Path;

type FrontCard = String;
type BackCard = String;
type StyleCard = String;

#[derive(Debug, PartialEq)]
pub struct Card {
  pub id: String,
  pub dependencies: Vec<String>,
  pub style: StyleCard,
  pub front: FrontCard,
  pub back: BackCard,
  pub deck: String,
}

fn parse_front_matter(text: &str) -> Option<(String, Vec<String>)> {
  let text = text.trim_start();
  if !text.starts_with("---") {
    return None;
  }

  let after_first = &text[3..];
  let end = after_first.find("---")?;
  let matter = after_first[..end].trim();

  let mut id = None;
  let mut deps = Vec::new();

  for line in matter.lines() {
    let line = line.trim();
    if let Some(val) = line.strip_prefix("id:") {
      id = Some(val.trim().to_string());
    } else if let Some(val) = line.strip_prefix("dependencies:") {
      let val = val.trim();
      if val == "[]" {
        deps = Vec::new();
      } else if val.starts_with('[') && val.ends_with(']') {
        deps = val[1..val.len() - 1]
          .split(',')
          .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
          .filter(|s| !s.is_empty())
          .collect();
      }
    }
  }

  Some((id?, deps))
}

fn strip_header<'a>(text: &'a str, header: &str) -> &'a str {
  for line in text.lines() {
    if line.trim() == header {
      let idx = text.find(line).unwrap() + line.len();
      return text[idx..].trim();
    }
  }
  text.trim()
}

fn md_to_html(input: &str) -> String {
  markdown::to_html_with_options(
    input,
    &markdown::Options {
      compile: markdown::CompileOptions {
        allow_dangerous_html: true,
        ..markdown::CompileOptions::default()
      },
      ..markdown::Options::default()
    },
  )
  .unwrap()
}

fn parse_card(raw: &str) -> Option<Card> {
  let (id, dependencies) = parse_front_matter(raw)?;

  let body = {
    let trimmed = raw.trim_start();
    let after_open = &trimmed[3..];
    let end = after_open.find("---").unwrap();
    &trimmed[3 + end + 3..]
  };

  let parts: Vec<&str> = body.splitn(3, "---").collect();

  let (style, front, back) = match parts.len() {
    3 => {
      let s = strip_header(parts[0], "# Style");
      let f = strip_header(parts[1], "# Front");
      let b = strip_header(parts[2], "# Back");
      (s.to_string(), f.to_string(), b.to_string())
    }
    2 => {
      let part0 = parts[0];
      if let Some(style_idx) = part0.find("# Style") {
        if let Some(front_idx) = part0.find("# Front") {
          if style_idx < front_idx {
            let s = strip_header(&part0[..front_idx], "# Style");
            let f = strip_header(&part0[front_idx..], "# Front");
            let b = strip_header(parts[1], "# Back");
            (s.to_string(), f.to_string(), b.to_string())
          } else {
            let f = strip_header(parts[0], "# Front");
            let b = strip_header(parts[1], "# Back");
            (String::new(), f.to_string(), b.to_string())
          }
        } else {
          let f = strip_header(parts[0], "# Front");
          let b = strip_header(parts[1], "# Back");
          (String::new(), f.to_string(), b.to_string())
        }
      } else {
        let f = strip_header(parts[0], "# Front");
        let b = strip_header(parts[1], "# Back");
        (String::new(), f.to_string(), b.to_string())
      }
    }
    _ => {
      eprintln!("Warning: '{}' missing '---' separator between front/back, skipping", id);
      return None;
    }
  };

  if front.is_empty() && back.is_empty() {
    return None;
  }

  Some(Card {
    id,
    dependencies,
    style,
    front: md_to_html(&front),
    back: md_to_html(&back),
    deck: String::new(),
  })
}

fn collect_md_files(dir: &Path, base: &Path, cards: &mut Vec<Card>, seen_ids: &mut HashSet<String>) -> Result<(), String> {
  let entries = fs::read_dir(dir).map_err(|e| format!("Error reading directory: {}", e))?;

  for entry in entries.flatten() {
    let path = entry.path();

    if path.is_dir() {
      collect_md_files(&path, base, cards, seen_ids)?;
    } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
      let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
          eprintln!("Error reading '{}': {}", path.display(), err);
          continue;
        }
      };

      match parse_card(&content) {
        Some(mut card) => {
          if !seen_ids.insert(card.id.clone()) {
            return Err(format!("Duplicate id '{}'", card.id));
          }

          let rel = path.parent().unwrap().strip_prefix(base).unwrap_or(Path::new(""));
          if !rel.as_os_str().is_empty() {
            card.deck = rel.components()
              .map(|c| c.as_os_str().to_string_lossy())
              .collect::<Vec<_>>()
              .join("::");
          }

          cards.push(card);
        }
        None => {
          eprintln!("Warning: '{}' has no front matter, skipping", path.display());
        }
      }
    }
  }

  Ok(())
}

pub fn parse_dir(dir_path: &String) -> Result<Vec<Card>, String> {
  let dir = Path::new(dir_path);
  if !dir.is_dir() {
    return Err(format!("'{}' is not a directory", dir_path));
  }

  let mut cards = Vec::new();
  let mut seen_ids = HashSet::new();

  collect_md_files(dir, dir, &mut cards, &mut seen_ids)?;

  Ok(cards)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_card(id: &str, deps: &[&str], front: &str, back: &str) -> Card {
    Card {
      id: id.to_string(),
      dependencies: deps.iter().map(|s| s.to_string()).collect(),
      style: String::new(),
      front: md_to_html(front),
      back: md_to_html(back),
      deck: String::new(),
    }
  }

  #[test]
  fn parse_basic_card() {
    let input = "---\nid: gg\ndependencies: []\n---\n\n# Front\n\nUm\n\n---\n\n# Back\n\nHey";
    let card = parse_card(input).unwrap();
    assert_eq!(card, make_card("gg", &[], "Um", "Hey"));
  }

  #[test]
  fn parse_card_with_deps() {
    let input = "---\nid: baz\ndependencies: [gg, foo]\n---\n\n# Front\n\nQ\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert_eq!(card, make_card("baz", &["gg", "foo"], "Q", "A"));
  }

  #[test]
  fn parse_card_no_front_matter() {
    let input = "# Front\n\nUm\n\n---\n\n# Back\n\nHey";
    assert!(parse_card(input).is_none());
  }

  #[test]
  fn parse_card_empty_body() {
    let input = "---\nid: empty\ndependencies: []\n---\n\n# Front\n\n---\n\n# Back\n\n";
    assert!(parse_card(input).is_none());
  }

  #[test]
  fn parse_card_with_style() {
    let input = "---\nid: styled\ndependencies: []\n---\n\n# Style\n\n.card { font-size: 20px; }\n\n# Front\n\nWhat?\n\n---\n\n# Back\n\nAnswer";
    let card = parse_card(input).unwrap();
    assert_eq!(card.id, "styled");
    assert_eq!(card.style, ".card { font-size: 20px; }");
    assert_eq!(card.front, md_to_html("What?"));
    assert_eq!(card.back, md_to_html("Answer"));
  }

  #[test]
  fn parse_card_with_multiline_style() {
    let input = "---\nid: styled2\ndependencies: []\n---\n\n# Style\n\n.card {\n  font-family: arial;\n  font-size: 20px;\n  text-align: center;\n  color: black;\n  background-color: white;\n}\n\n.nightMode .card {\n  background-color: #333;\n}\n\n# Front\n\nQ\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert!(card.style.contains(".card {"));
    assert!(card.style.contains(".nightMode"));
  }

  #[test]
  fn duplicate_ids_detected() {
    let mut seen = HashSet::new();
    assert!(seen.insert("dup".to_string()));
    assert!(!seen.insert("dup".to_string()));
  }
}
