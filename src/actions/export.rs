use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use sha1::{Digest, Sha1};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::build::Card;
use super::build::load_deck_template;
use super::build::parse_dir;
use super::widget;
use crate::config;

fn id_from_name(name: &str) -> i64 {
  let mut hasher = Sha1::new();
  hasher.update(name.as_bytes());
  let result = hasher.finalize();
  let mut num: i64 = 0;
  for byte in result.iter().take(8) {
    num = (num << 8) | (*byte as i64);
  }
  num.abs()
}

fn timestamp_ms() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_millis() as i64
}

fn timestamp_secs() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64
}

fn generate_guid(deck: &str, card_id: &str) -> String {
  let hash_input = if deck.is_empty() {
    card_id.to_string()
  } else {
    format!("{}::{}", deck, card_id)
  };
  let mut hasher = Sha1::new();
  hasher.update(hash_input.as_bytes());
  let result = hasher.finalize();

  const CHARSET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,./:;<=>?@[]^_`{|}~\"";
  let mut num: u64 = 0;
  for byte in result.iter().take(8) {
    num = (num << 8) | (*byte as u64);
  }

  let mut guid = String::with_capacity(10);
  for _ in 0..10 {
    guid.push(CHARSET[(num % 91) as usize] as char);
    num /= 91;
  }
  guid
}

fn checksum(data: &str) -> u32 {
  let mut hasher = Sha1::new();
  hasher.update(data.as_bytes());
  let result = hasher.finalize();
  let hex_str: String = result
    .iter()
    .take(4)
    .map(|b| format!("{:02x}", b))
    .collect();
  u32::from_str_radix(&hex_str, 16).unwrap_or(0)
}

fn combine_css(shared_css: &str) -> String {
  let default_css = "\
.card {
  font-family: arial;
  font-size: 20px;
  text-align: center;
  color: black;
  background-color: white;
}";

  if shared_css.is_empty() {
    default_css.to_string()
  } else {
    format!("{}\n\n{}", default_css, shared_css)
  }
}

fn resolve_shared_css(root: &Path, cfg: &Option<config::AnkiDaikuConfig>) -> String {
  if let Some(cfg) = cfg {
    if let Some(path) = &cfg.shared_css {
      let full_path = root.join(path);
      if let Ok(content) = fs::read_to_string(&full_path) {
        return content;
      }
    }
  }

  let auto_path = root.join("shared.css");
  fs::read_to_string(auto_path).unwrap_or_default()
}

fn wrap_style(content: &str, style: &str) -> String {
  if style.is_empty() {
    content.to_string()
  } else {
    format!("<style>\n{}\n</style>\n{}", style, content)
  }
}

/// Front template (the "Type" shell, deck-wide). Renders the card's question
/// and, when the card has `# Type` answers, the per-card `Widget` field that
/// our build produces. Decks can fully replace this shell via `template.yaml`;
/// the widget itself is user-extensible at the deck / component / card level.
const QFMT: &str = r##"{{Front}}
{{#Type}}
{{Widget}}
{{/Type}}"##;

fn extract_media_paths(html: &str) -> Vec<String> {
  let mut paths = Vec::new();
  let bytes = html.as_bytes();
  let len = bytes.len();
  let mut i = 0;

  while i < len {
    if i + 4 <= len && &bytes[i..i + 4] == b"src=" {
      i += 4;
      while i < len && bytes[i] == b' ' {
        i += 1;
      }
      if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
        let quote = bytes[i];
        i += 1;
        let start = i;
        while i < len && bytes[i] != quote {
          i += 1;
        }
        let path = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
        if !path.is_empty()
          && !path.starts_with("http://")
          && !path.starts_with("https://")
          && !path.starts_with("data:")
        {
          paths.push(path.to_string());
        }
      }
    } else {
      i += 1;
    }
  }

  paths
}

fn collect_media(
  cards: &[Card],
  media_dir: &Path,
  templates: &[&str],
) -> Result<(HashMap<String, u32>, HashMap<i64, Vec<u8>>), Box<dyn std::error::Error>> {
  let mut name_to_id: HashMap<String, u32> = HashMap::new();
  let mut id_to_data: HashMap<i64, Vec<u8>> = HashMap::new();
  let mut path_to_resolved: HashMap<String, String> = HashMap::new();
  let mut resolved_to_id_name: HashMap<String, String> = HashMap::new();

  for html in templates.iter().copied().chain(
    cards
      .iter()
      .flat_map(|card| [card.front.as_str(), card.back.as_str(), card.widget.as_str()]),
  ) {
    for path in extract_media_paths(html) {
      let actual = media_dir.join(&path);

      if !actual.exists() {
        return Err(
          format!(
            "Media file '{}' not found in '{}'",
            path,
            media_dir.display()
          )
          .into(),
        );
      }

      let actual_str = actual.to_string_lossy().to_string();

      if let Some(prev) = path_to_resolved.get(&path) {
        if *prev != actual_str {
          return Err(
            format!(
              "Media collision: '{}' resolves to both '{}' and '{}'",
              path, prev, actual_str
            )
            .into(),
          );
        }
        continue;
      }

      if let Some(existing_name) = resolved_to_id_name.get(&actual_str) {
        path_to_resolved.insert(path.clone(), actual_str);
        name_to_id.insert(path.clone(), name_to_id[existing_name]);
        continue;
      }

      path_to_resolved.insert(path.clone(), actual_str.clone());
      resolved_to_id_name.insert(actual_str, path.clone());

      let data = fs::read(&actual)?;
      let id = name_to_id.len() as u32;
      name_to_id.insert(path.clone(), id);
      id_to_data.insert(id as i64, data);
    }
  }

  Ok((name_to_id, id_to_data))
}

fn resolve_media_dir(root: &Path, cfg: &Option<config::AnkiDaikuConfig>) -> std::path::PathBuf {
  cfg
    .as_ref()
    .and_then(|c| c.media_dir.as_ref())
    .map(|d| root.join(d))
    .unwrap_or_else(|| root.join("media"))
}

fn format_size(bytes: u64) -> String {
  if bytes < 1024 {
    format!("{} B", bytes)
  } else if bytes < 1024 * 1024 {
    format!("{:.1} KB", bytes as f64 / 1024.0)
  } else {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
  }
}

const MANIFEST_DIR: &str = ".ankidaiku";
const MANIFEST_FILE: &str = "manifest.json";

fn load_manifest(root: &Path) -> HashMap<String, String> {
  let path = root.join(MANIFEST_DIR).join(MANIFEST_FILE);
  let content = match fs::read_to_string(&path) {
    Ok(c) => c,
    Err(_) => return HashMap::new(),
  };
  match serde_json::from_str::<serde_json::Value>(&content) {
    Ok(val) => val
      .get("cards")
      .and_then(|c| c.as_object())
      .map(|obj| {
        obj
          .iter()
          .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
          .collect()
      })
      .unwrap_or_else(|| serde_json::from_value(val).unwrap_or_default()),
    Err(_) => HashMap::new(),
  }
}

fn save_manifest(
  root: &Path,
  manifest: &BTreeMap<String, serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
  let dir = root.join(MANIFEST_DIR);
  fs::create_dir_all(&dir)?;
  let content = serde_json::to_string_pretty(manifest)?;
  fs::write(dir.join(MANIFEST_FILE), content)?;
  Ok(())
}

fn prompt_confirm(msg: &str) -> bool {
  eprint!("{} [y/N] ", msg);
  let mut input = String::new();
  std::io::stdin().read_line(&mut input).ok();
  matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

fn build_decks(
  root_name: &str,
  desc: &str,
  cards: &[Card],
) -> (HashMap<String, i64>, serde_json::Value) {
  let now = timestamp_secs();
  let dconf_id = 1;
  let mut paths = std::collections::HashSet::new();

  for card in cards {
    if card.deck.is_empty() {
      paths.insert(root_name.to_string());
    } else {
      paths.insert(format!("{}::{}", root_name, card.deck));
    }
  }

  let mut all = std::collections::HashSet::new();
  for path in &paths {
    let parts: Vec<&str> = path.split("::").collect();
    for i in 0..parts.len() {
      all.insert(parts[..=i].join("::"));
    }
  }

  let mut id_map = HashMap::new();
  let mut decks_map = serde_json::Map::new();

  for path in &all {
    let did = id_from_name(&format!("deck_{}", path));
    id_map.insert(path.clone(), did);
  }

  for (path, &did) in &id_map {
    let is_root = *path == *root_name;
    decks_map.insert(
      did.to_string(),
      serde_json::json!({
        "id": did,
        "name": path,
        "mod": now,
        "usn": -1,
        "lrnToday": [0, 0],
        "revToday": [0, 0],
        "newToday": [0, 0],
        "timeToday": [0, 0],
        "collapsed": false,
        "browserCollapsed": false,
        "desc": if is_root { desc } else { "" },
        "dyn": 0,
        "conf": dconf_id,
        "extendNew": 0,
        "extendRev": 0
      }),
    );
  }

  (id_map, serde_json::Value::Object(decks_map))
}

pub fn resolve_cards_dir(
  root: &Path,
  cli_cards_dir: Option<&str>,
  config_cards_dir: Option<&str>,
) -> String {
  if let Some(dir) = cli_cards_dir {
    return root.join(dir).to_string_lossy().to_string();
  }
  if let Some(dir) = config_cards_dir {
    return root.join(dir).to_string_lossy().to_string();
  }
  root.join("cards").to_string_lossy().to_string()
}

pub fn resolve_config(root: &Path, config_path: Option<&str>) -> Option<config::AnkiDaikuConfig> {
  match config_path {
    Some(path) => {
      let p = Path::new(path);
      if p.is_absolute() {
        config::parse_config_path(p)
      } else {
        config::parse_config_path(&root.join(path))
      }
    }
    None => config::parse_config(root),
  }
}

fn deck_name(pkg: &Option<config::PackageJson>) -> String {
  pkg
    .as_ref()
    .and_then(|p| p.name.clone())
    .unwrap_or_else(|| "AnkiDeck".to_string())
}

fn deck_desc(pkg: &Option<config::PackageJson>) -> String {
  pkg
    .as_ref()
    .and_then(|p| p.description.clone())
    .unwrap_or_default()
}

pub fn export_apkg(
  dir_path: &str,
  output_path: &str,
  cli_cards_dir: Option<&str>,
  config_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
  let root = Path::new(dir_path);

  let pkg = config::parse_package_json(root);
  let cfg = resolve_config(root, config_path);
  let cards_dir = resolve_cards_dir(
    root,
    cli_cards_dir,
    cfg.as_ref().and_then(|c| c.cards_dir.as_deref()),
  );

  let name = deck_name(&pkg);
  let desc = deck_desc(&pkg);
  let model_id = id_from_name(&name);
  let dconf_id = 1;

  let mut cards = parse_dir(&cards_dir).map_err(|e| format!("Build error: {}", e))?;

  // Resolve each card's widget (deck/component/card level) into the `Widget`
  // field. This must happen before media scanning so custom widgets are seen.
  for card in &mut cards {
    card.widget = widget::resolve_widget(card, Path::new(&cards_dir))
      .map_err(|e| format!("Build error: {}", e))?;
  }

  // Optional deck-supplied template. When absent, the built-in model/QFMT is
  // used unchanged. When present, the deck controls the card template, the
  // extra fields, and (optionally) extra css.
  let deck_template =
    load_deck_template(Path::new(&cards_dir)).map_err(|e| format!("Build error: {}", e))?;
  let (qfmt, afmt, model_fields, plain_fields, template_css) =
    {
      let mut fields: Vec<String> = vec![
        "Front".into(),
        "Back".into(),
        "Type".into(),
        "Widget".into(),
      ];
      let mut declared: HashSet<String> = fields.iter().cloned().collect();
      let mut plain: HashSet<String> = HashSet::new();
      plain.insert("Type".into());
      let mut css = String::new();
      if let Some(tpl) = &deck_template {
        for f in &tpl.fields {
          if !declared.insert(f.name.clone()) {
            return Err(
              format!(
                "Build error: template field '{}' duplicates a built-in field",
                f.name
              )
              .into(),
            );
          }
          if f.plain {
            plain.insert(f.name.clone());
          }
          fields.push(f.name.clone());
        }
        for card in &cards {
          for (name, _) in &card.extras {
            if !declared.contains(name) {
              return Err(format!(
              "Build error: card '{}' uses '# {}' but field is not declared in the deck template",
              card.id, name
            ).into());
            }
          }
        }
        if let Some(c) = &tpl.css {
          css = c.clone();
        }
      }
      let tpl_qfmt = deck_template.as_ref().and_then(|t| t.qfmt.clone());
      let tpl_afmt = deck_template.as_ref().and_then(|t| t.afmt.clone());
      (
        tpl_qfmt.unwrap_or_else(|| QFMT.to_string()),
        tpl_afmt.unwrap_or_else(|| "{{FrontSide}}<hr id=\"answer\">{{Back}}".to_string()),
        fields,
        plain,
        css,
      )
    };

  let prev = load_manifest(root);
  let manifest_key = |card: &Card| -> String {
    if card.deck.is_empty() {
      card.id.clone()
    } else {
      format!("{}::{}", card.deck, card.id)
    }
  };
  let current_keys: HashSet<_> = cards.iter().map(|c| manifest_key(c)).collect();
  let removed: Vec<_> = prev.keys().filter(|k| !current_keys.contains(*k)).collect();

  if !removed.is_empty() {
    eprintln!(
      "Warning: {} card(s) from the previous export were not found:",
      removed.len()
    );
    for id in &removed {
      eprintln!("  - {}", id);
    }
    eprintln!("These cards will remain orphaned in Anki.");
    if !prompt_confirm("Continue?") {
      eprintln!("Aborted.");
      return Ok(());
    }
  }

  let (deck_ids, decks_json) = build_decks(&name, &desc, &cards);
  let root_deck_id = deck_ids
    .get(&name)
    .copied()
    .unwrap_or_else(|| id_from_name(&format!("deck_{}", name)));

  let db_path = std::env::temp_dir().join(format!("anki_daiku_{}.db", timestamp_ms()));
  let shared_css = resolve_shared_css(root, &cfg);
  let css = if template_css.is_empty() {
    combine_css(&shared_css)
  } else {
    format!("{}\n\n{}", combine_css(&shared_css), template_css)
  };

  let media_dir = resolve_media_dir(root, &cfg);
  let (media_map, media_data) = collect_media(&cards, &media_dir, &[&qfmt, &afmt])?;

  let media_path_to_basename: HashMap<&str, &str> = media_map
    .keys()
    .map(|p| {
      (
        p.as_str(),
        Path::new(p).file_name().unwrap().to_str().unwrap(),
      )
    })
    .collect();

  {
    let conn = Connection::open(&db_path)?;

    conn.execute_batch(
      "CREATE TABLE IF NOT EXISTS col (
        id integer primary key,
        crt integer not null,
        mod integer not null,
        scm integer not null,
        ver integer not null,
        dty integer not null,
        usn integer not null,
        ls integer not null,
        conf text not null,
        models text not null,
        decks text not null,
        dconf text not null,
        tags text not null
      );
      CREATE TABLE IF NOT EXISTS notes (
        id integer primary key,
        guid text not null,
        mid integer not null,
        mod integer not null,
        usn integer not null,
        tags text not null,
        flds text not null,
        sfld text not null,
        csum integer not null,
        flags integer not null,
        data text not null
      );
      CREATE TABLE IF NOT EXISTS cards (
        id integer primary key,
        nid integer not null,
        did integer not null,
        ord integer not null,
        mod integer not null,
        usn integer not null,
        type integer not null,
        queue integer not null,
        due integer not null,
        ivl integer not null,
        factor integer not null,
        reps integer not null,
        lapses integer not null,
        left integer not null,
        odue integer not null,
        odid integer not null,
        flags integer not null,
        data text not null
      );
      CREATE TABLE IF NOT EXISTS revlog (
        id integer primary key,
        cid integer not null,
        usn integer not null,
        ease integer not null,
        ivl integer not null,
        lastIvl integer not null,
        factor integer not null,
        time integer not null,
        type integer not null
      );
      CREATE TABLE IF NOT EXISTS graves (
        usn integer not null,
        oid integer not null,
        type integer not null
      );",
    )?;

    let now_mod = timestamp_secs();

    let model_flds_json = serde_json::Value::Array(
      model_fields
        .iter()
        .enumerate()
        .map(|(ord, f)| {
          serde_json::json!({
            "name": f,
            "ord": ord,
            "sticky": false,
            "rtl": false,
            "font": "Arial",
            "size": 20,
            "media": [],
            "description": "",
            "plainText": plain_fields.contains(f)
          })
        })
        .collect(),
    );

    let models_json = serde_json::json!({
      model_id.to_string(): {
        "id": model_id,
        "name": &name,
        "type": 0,
        "mod": now_mod,
        "usn": -1,
        "sortf": 0,
        "did": root_deck_id,
        "tmpls": [{
          "name": "Card 1",
          "qfmt": qfmt,
          "afmt": afmt,
          "ord": 0,
          "bqfmt": "",
          "bafmt": ""
        }],
        "flds": model_flds_json,
        "css": css,
        "latexPre": "",
        "latexPost": "",
        "latexSvg": false,
        "req": [[0, "any", [0]]]
      }
    });

    let dconf_json = serde_json::json!({
      dconf_id.to_string(): {
        "id": dconf_id,
        "mod": 0,
        "name": "Default",
        "usn": 0,
        "maxTaken": 60,
        "autoplay": true,
        "timer": 0,
        "replayq": true,
        "new": {
          "bury": true,
          "delays": [1.0, 10.0],
          "ints": [1, 4, 0],
          "order": 1,
          "perDay": 20
        },
        "rev": {
          "bury": true,
          "ease4": 1.3,
          "ivlFct": 1.0,
          "maxIvl": 36500,
          "perDay": 200,
          "hardFactor": 1.2,
          "fuzz": 0.05
        },
        "lapse": {
          "delays": [10.0],
          "leechAction": 1,
          "leechFails": 8,
          "minInt": 1,
          "mult": 0.0
        },
        "maxTime": 60
      }
    });

    let conf_json = serde_json::json!({
      "activeDecks": [root_deck_id],
      "curDeck": root_deck_id,
      "newSpread": 0,
      "collapseTime": 1200,
      "timeLim": 0,
      "estTimes": true,
      "dueCounts": true,
      "curModel": model_id,
      "nextPos": 1,
      "sortType": "noteFld",
      "sortBackwards": false,
      "addToCur": true,
      "dayLearnFirst": false,
      "schedVer": 1
    });

    let now_ms = timestamp_ms();

    conn.execute(
      "INSERT INTO col (id, crt, mod, scm, ver, dty, usn, ls, conf, models, decks, dconf, tags)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
      params![
        1,
        now_mod,
        now_ms,
        now_ms,
        11,
        0,
        -1,
        0,
        conf_json.to_string(),
        models_json.to_string(),
        decks_json.to_string(),
        dconf_json.to_string(),
        "{}"
      ],
    )?;

    for (i, card) in cards.iter().enumerate() {
      let note_id = now_ms + (i as i64) * 2;
      let card_id = note_id + 1;
      let guid = generate_guid(&card.deck, &card.id);

      let front = media_path_to_basename
        .iter()
        .fold(card.front.clone(), |acc, (path, basename)| {
          acc.replace(path, basename)
        });
      let back = media_path_to_basename
        .iter()
        .fold(card.back.clone(), |acc, (path, basename)| {
          acc.replace(path, basename)
        });
      let widget = media_path_to_basename
        .iter()
        .fold(card.widget.clone(), |acc, (path, basename)| {
          acc.replace(path, basename)
        });
      let wrapped_front = wrap_style(&front, &card.style);
      let wrapped_back = wrap_style(&back, &card.style);
      let extra_lookup: HashMap<&str, &str> = card
        .extras
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
      let flds = model_fields
        .iter()
        .map(|f| match f.as_str() {
          "Front" => wrapped_front.as_str(),
          "Back" => wrapped_back.as_str(),
          "Type" => card.typed.as_str(),
          "Widget" => widget.as_str(),
          other => extra_lookup.get(other).copied().unwrap_or(""),
        })
        .collect::<Vec<_>>()
        .join("\x1f");
      let sfld = front.clone();
      let csum = checksum(&sfld);
      let tags = card.dependencies.join(" ");

      conn.execute(
        "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
          note_id, guid, model_id, now_mod, -1, tags, flds, sfld, csum, 0, "",
        ],
      )?;

      let card_deck = if card.deck.is_empty() {
        root_deck_id
      } else {
        let path = format!("{}::{}", name, card.deck);
        *deck_ids.get(&path).unwrap_or(&root_deck_id)
      };

      conn.execute(
        "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
          card_id, note_id, card_deck, 0, now_ms, -1, 0, 0, (i as i64) + 1, 0, 0, 0, 0, 0, 0, 0, 0,
          "{\"pos\":0}",
        ],
      )?;
    }

    conn.close().map_err(|e| e.1)?;
  }

  let output = Path::new(output_path);
  if let Some(parent) = output.parent() {
    fs::create_dir_all(parent)?;
  }

  let file = File::create(output)?;
  let mut zip = ZipWriter::new(file);
  let options = SimpleFileOptions::default()
    .compression_method(zip::CompressionMethod::Deflated)
    .compression_level(Some(6));

  let mut db_file = File::open(&db_path)?;
  let mut db_bytes = Vec::new();
  db_file.read_to_end(&mut db_bytes)?;
  zip.start_file("collection.anki2", options)?;
  zip.write_all(&db_bytes)?;

  for (id, data) in &media_data {
    zip.start_file(id.to_string(), options)?;
    zip.write_all(data)?;
  }

  let mut media_json: HashMap<String, String> = HashMap::new();
  for (filename, id) in &media_map {
    let basename = Path::new(filename)
      .file_name()
      .unwrap()
      .to_str()
      .unwrap()
      .to_string();
    media_json.insert(id.to_string(), basename);
  }
  let media_opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
  zip.start_file("media", media_opts)?;
  zip.write_all(serde_json::to_string(&media_json)?.as_bytes())?;

  zip.finish()?;

  fs::remove_file(&db_path)?;

  let file_size = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);

  let total = cards.len();
  let deleted_count = cards.iter().filter(|c| c.deleted).count();
  let active_count = total - deleted_count;
  let media_count = media_data.len();
  print!(
    "Exported {} card(s) ({} active, {} deleted)",
    total, active_count, deleted_count
  );
  if media_count > 0 {
    print!(", {} media file(s)", media_count);
  }
  println!(" to '{}' ({})", output_path, format_size(file_size));

  let mut manifest: BTreeMap<String, serde_json::Value> = BTreeMap::new();
  manifest.insert(
    "version".to_string(),
    serde_json::Value::String("1".to_string()),
  );
  let mut cards_map = serde_json::Map::new();
  for card in &cards {
    let key = if card.deck.is_empty() {
      card.id.clone()
    } else {
      format!("{}::{}", card.deck, card.id)
    };
    cards_map.insert(
      key,
      serde_json::Value::String(generate_guid(&card.deck, &card.id)),
    );
  }
  manifest.insert("cards".to_string(), serde_json::Value::Object(cards_map));
  save_manifest(root, &manifest)?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn generate_guid_returns_10_chars() {
    let guid = generate_guid("", "test-card");
    assert_eq!(guid.len(), 10);
  }

  #[test]
  fn generate_guid_is_deterministic() {
    let a = generate_guid("deck1", "hello");
    let b = generate_guid("deck1", "hello");
    assert_eq!(a, b);
  }

  #[test]
  fn generate_guid_differs_for_different_ids() {
    let a = generate_guid("deck1", "card-1");
    let b = generate_guid("deck1", "card-2");
    assert_ne!(a, b);
  }

  #[test]
  fn generate_guid_differs_for_different_decks() {
    let a = generate_guid("Math", "card-1");
    let b = generate_guid("Science", "card-1");
    assert_ne!(a, b);
  }

  #[test]
  fn checksum_is_deterministic() {
    let a = checksum("hello");
    let b = checksum("hello");
    assert_eq!(a, b);
  }

  #[test]
  fn checksum_differs_for_different_input() {
    let a = checksum("hello");
    let b = checksum("world");
    assert_ne!(a, b);
  }
}
