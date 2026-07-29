use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use sha1::{Digest, Sha1};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use super::build::{parse_dir, Card};

const MODEL_ID: i64 = 1607392319;
const DECK_ID: i64 = 1;
const DCONF_ID: i64 = 1;

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

fn generate_guid(card_id: &str) -> String {
  let mut hasher = Sha1::new();
  hasher.update(card_id.as_bytes());
  let result = hasher.finalize();

  const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,./:;<=>?@[]^_`{|}~\"";
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
  let hex_str: String = result.iter().take(4).map(|b| format!("{:02x}", b)).collect();
  u32::from_str_radix(&hex_str, 16).unwrap_or(0)
}

fn combine_css(cards: &[Card]) -> String {
  let default_css = "\
.card {
  font-family: arial;
  font-size: 20px;
  text-align: center;
  color: black;
  background-color: white;
}";

  let mut parts = vec![default_css.to_string()];

  for card in cards {
    if !card.style.is_empty() {
      parts.push(card.style.clone());
    }
  }

  parts.join("\n\n")
}

pub fn export_apkg(dir_path: &str, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
  let cards = parse_dir(&dir_path.to_string());

  if cards.is_empty() {
    eprintln!("No cards found in '{}'", dir_path);
    return Ok(());
  }

  let db_path = std::env::temp_dir().join(format!("anki_daiku_{}.db", timestamp_ms()));
  let css = combine_css(&cards);

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

    let models_json = serde_json::json!({
      MODEL_ID.to_string(): {
        "id": MODEL_ID,
        "name": "AnkiDaiku",
        "type": 0,
        "mod": 0,
        "usn": -1,
        "sortf": 0,
        "did": DECK_ID,
        "tmpls": [{
          "name": "Card 1",
          "qfmt": "{{Front}}",
          "afmt": "{{FrontSide}}<hr id=\"answer\">{{Back}}",
          "ord": 0,
          "bqfmt": "",
          "bafmt": ""
        }],
        "flds": [{
          "name": "Front",
          "ord": 0,
          "sticky": false,
          "rtl": false,
          "font": "Arial",
          "size": 20,
          "media": [],
          "description": "",
          "plainText": false
        }, {
          "name": "Back",
          "ord": 1,
          "sticky": false,
          "rtl": false,
          "font": "Arial",
          "size": 20,
          "media": [],
          "description": "",
          "plainText": false
        }],
        "css": css,
        "latexPre": "",
        "latexPost": "",
        "latexSvg": false,
        "req": [[0, "any", [0]]]
      }
    });

    let decks_json = serde_json::json!({
      DECK_ID.to_string(): {
        "id": DECK_ID,
        "name": "AnkiDaiku",
        "mod": 0,
        "usn": -1,
        "lrnToday": [0, 0],
        "revToday": [0, 0],
        "newToday": [0, 0],
        "timeToday": [0, 0],
        "collapsed": false,
        "browserCollapsed": false,
        "desc": "",
        "dyn": 0,
        "conf": DCONF_ID,
        "extendNew": 0,
        "extendRev": 0
      }
    });

    let dconf_json = serde_json::json!({
      DCONF_ID.to_string(): {
        "id": DCONF_ID,
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
      "activeDecks": [DECK_ID],
      "curDeck": DECK_ID,
      "newSpread": 0,
      "collapseTime": 1200,
      "timeLim": 0,
      "estTimes": true,
      "dueCounts": true,
      "curModel": MODEL_ID,
      "nextPos": 1,
      "sortType": "noteFld",
      "sortBackwards": false,
      "addToCur": true,
      "dayLearnFirst": false,
      "schedVer": 1
    });

    let crt = timestamp_secs();
    let now_ms = timestamp_ms();

    conn.execute(
      "INSERT INTO col (id, crt, mod, scm, ver, dty, usn, ls, conf, models, decks, dconf, tags)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
      params![
        1,
        crt,
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
      let guid = generate_guid(&card.id);
      let flds = format!("{}\x1f{}", card.front, card.back);
      let sfld = card.front.clone();
      let csum = checksum(&sfld);
      let tags = card.dependencies.join(" ");

      conn.execute(
        "INSERT INTO notes (id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![note_id, guid, MODEL_ID, crt, -1, tags, flds, sfld, csum, 0, "",],
      )?;

      conn.execute(
        "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
          card_id, note_id, DECK_ID, 0, now_ms, -1, 0, 0, (i as i64) + 1, 0, 0, 0, 0, 0, 0, 0, 0,
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

  let media_json = serde_json::json!({});
  zip.start_file("media", SimpleFileOptions::default())?;
  zip.write_all(media_json.to_string().as_bytes())?;

  zip.finish()?;

  fs::remove_file(&db_path)?;

  println!("Exported {} card(s) to '{}'", cards.len(), output_path);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn generate_guid_returns_10_chars() {
    let guid = generate_guid("test-card");
    assert_eq!(guid.len(), 10);
  }

  #[test]
  fn generate_guid_is_deterministic() {
    let a = generate_guid("hello");
    let b = generate_guid("hello");
    assert_eq!(a, b);
  }

  #[test]
  fn generate_guid_differs_for_different_ids() {
    let a = generate_guid("card-1");
    let b = generate_guid("card-2");
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
