use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use serde::Deserialize;

type FrontCard = String;
type BackCard = String;
type StyleCard = String;

/// Deck-supplied schema (`cards/schema.yaml`) that describes the data file the
/// deck drives component cards from. The builder rejects records that don't
/// validate; every field stays deck-shaped (no built-in knowledge of words).
#[derive(Debug, Deserialize)]
pub struct PipelineSchema {
  pub data: DataSpec,
  #[serde(default = "default_templates_dir")]
  pub templates_dir: String,
  #[serde(default)]
  pub record: RecordSpec,
}

#[derive(Debug, Deserialize)]
pub struct DataSpec {
  /// Data file relative to the cards dir.
  pub file: String,
  /// Root key holding the list of records.
  pub list: String,
  /// Field on each record listing which component templates to fan out to.
  pub components: String,
}

fn default_templates_dir() -> String {
  "types".to_string()
}

/// Declared shape of a record, used for dynamic validation.
#[derive(Debug, Deserialize, Default)]
pub struct RecordSpec {
  /// Fields that must be present and non-empty.
  #[serde(default)]
  pub required: Vec<String>,
  /// Typed fields (everything unlisted is treated as an optional string).
  #[serde(default)]
  pub types: std::collections::BTreeMap<String, FieldType>,
}

#[derive(Debug)]
pub enum FieldType {
  /// `list` of plain scalars.
  List,
  /// Nested record list, e.g. `{ list: [{ name: the }, { name: syns, type: list }] }`.
  ListOfRecords(ListOfRecords),
}

#[derive(Debug, Deserialize)]
pub struct ListOfRecords {
  pub list: Vec<SubField>,
}

impl<'de> serde::Deserialize<'de> for FieldType {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let value = <serde_yaml::Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(match value {
      serde_yaml::Value::String(s) if s == "list" => FieldType::List,
      other => {
        let lor: ListOfRecords =
          serde_yaml::from_value(other).map_err(|e| serde::de::Error::custom(format!("invalid field type: {}", e)))?;
        FieldType::ListOfRecords(lor)
      }
    })
  }
}

#[derive(Debug, Deserialize)]
pub struct SubField {
  pub name: String,
  #[serde(rename = "type")]
  pub inner: Option<FieldType>,
}

#[derive(Debug, Clone)]
pub struct ComponentTemplate {
  pub name: String,
  pub deck: String,
  pub id: String,
  pub widget: String,
  pub body: String,
}

#[derive(Debug, PartialEq)]
pub struct Card {
  pub id: String,
  pub dependencies: Vec<String>,
  pub style: StyleCard,
  pub front: FrontCard,
  pub back: BackCard,
  pub typed: String,
  /// Optional `# Answers` section: overrides the value the JS widget receives
  /// for `{{answers}}`. Falls back to `typed` when empty.
  pub answers: String,
  /// Inline widget HTML from a `# Widget` section. Set on the card, or from
  /// the `# Widget` section of the component type that generated it.
  pub widget: String,
  /// A named widget (`cards/widgets/<name>.html`) referenced via the `widget:`
  /// front-matter key on the card or its component type.
  pub widget_name: String,
  /// Extra `# Section` values beyond Front/Back/Type, in document order.
  pub extras: Vec<(String, String)>,
  pub deck: String,
  pub deleted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DeckTemplate {
  pub qfmt: Option<String>,
  pub afmt: Option<String>,
  pub css: Option<String>,
  pub fields: Vec<TemplateField>,
}

#[derive(Debug, Clone, Default)]
pub struct TemplateField {
  pub name: String,
  pub plain: bool,
}

/// Read an optional `template.yaml` next to the cards. It lets a deck ship its
/// own card template (qfmt/afmt), extra notetype fields, and per-deck css.
pub fn load_deck_template(cards_dir: &Path) -> Result<Option<DeckTemplate>, String> {
  let mut path = cards_dir.join("template.yaml");
  if !path.is_file() {
    path = cards_dir.join("template.yml");
  }
  if !path.is_file() {
    return Ok(None);
  }
  let content = fs::read_to_string(&path).map_err(|e| format!("Error reading '{}': {}", path.display(), e))?;

  #[derive(Deserialize, Default)]
  struct RawDeckTemplate {
    qfmt: Option<String>,
    afmt: Option<String>,
    css: Option<String>,
    fields: Option<Vec<RawField>>,
  }

  #[derive(Deserialize, Default)]
  struct RawField {
    name: Option<String>,
    #[serde(default)]
    plain: bool,
  }

  let raw: RawDeckTemplate = serde_yaml::from_str(&content)
    .map_err(|e| format!("Error parsing '{}': {}", path.display(), e))?;

  let mut fields = Vec::new();
  if let Some(raw_fields) = raw.fields {
    for f in raw_fields {
      let name = f.name.ok_or_else(|| format!(
        "Error in '{}': every template field needs a 'name'",
        path.display()
      ))?;
      let name = name.trim().to_string();
      if name.is_empty() {
        return Err(format!("Error in '{}': empty field name", path.display()));
      }
      fields.push(TemplateField { name, plain: f.plain });
    }
  }

  Ok(Some(DeckTemplate {
    qfmt: raw.qfmt.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    afmt: raw.afmt.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    css: raw.css.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
    fields,
  }))
}

fn parse_front_matter(text: &str) -> Option<(String, Vec<String>, String)> {
  let text = text.trim_start();
  if !text.starts_with("---") {
    return None;
  }

  let after_first = &text[3..];
  let end = after_first.find("---")?;
  let matter = after_first[..end].trim();

  let mut id = None;
  let mut deps = Vec::new();
  let mut widget = String::new();

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
    } else if let Some(val) = line.strip_prefix("widget:") {
      widget = val.trim().to_string();
    }
  }

  Some((id?, deps, widget))
}

/// Split a card body into its sections. Besides the built-in
/// `# Style`, `# Front`, `# Back`, `# Type`, `# Answers`, `# Widget` headers,
/// any other `# Name` header becomes an extra field (stored as raw text, like
/// `# Type`).
fn split_sections(
  body: &str,
) -> (String, String, String, String, String, String, Vec<(String, String)>) {
  let mut style = String::new();
  let mut front = String::new();
  let mut back = String::new();
  let mut typed = String::new();
  let mut answers = String::new();
  let mut widget = String::new();
  let mut extras: Vec<(String, String)> = Vec::new();
  // 0 = none, 1 = Style, 2 = Front, 3 = Back, 4 = Type, 5 = Answers, 6 = Widget, 7 = extra
  let mut section = 0usize;
  let mut extra_name = String::new();

  for chunk in body.split("---") {
    for line in chunk.lines() {
      match line.trim() {
        "# Style" => {
          section = 1;
        }
        "# Front" => {
          section = 2;
        }
        "# Back" => {
          section = 3;
        }
        "# Type" => {
          section = 4;
        }
        "# Answers" => {
          section = 5;
        }
        "# Widget" => {
          section = 6;
        }
        line if line.starts_with("# ") => {
          section = 7;
          extra_name = line[2..].trim().to_string();
        }
        _ => {
          let sink: &mut String = match section {
            1 => &mut style,
            2 => &mut front,
            3 => &mut back,
            4 => &mut typed,
            5 => &mut answers,
            6 => &mut widget,
            7 => {
              if extras.last().map(|(name, _)| *name == extra_name).unwrap_or(false) {
                &mut extras.last_mut().unwrap().1
              } else {
                extras.push((extra_name.clone(), String::new()));
                &mut extras.last_mut().unwrap().1
              }
            }
            _ => continue,
          };
          sink.push_str(line);
          sink.push('\n');
        }
      }
    }
  }

  (
    style.trim().to_string(),
    front.trim().to_string(),
    back.trim().to_string(),
    typed.trim().to_string(),
    answers.trim().to_string(),
    widget.trim().to_string(),
    extras
      .into_iter()
      .map(|(name, value)| (name, value.trim().to_string()))
      .collect(),
  )
}

/// One record's data, exposed to component templates as a scope stack. The
/// innermost value is the current record (or the current `#each` item).
type Scope = Vec<serde_yaml::Value>;

/// Render `tpl` against `scope`: plain `{{path}}` substitution, `#if/#else/`
/// blocks, `#each` loops over lists (with `../` for the outer record), and the
/// small fixed function set `join`, `join_html`, `fallback`, `letters`,
/// `upper_first`, `underline`, `replace`.
fn render_template(tpl: &str, scope: &Scope) -> String {
  let tokens = tokenize_template(tpl);
  let mut i = 0usize;
  let nodes = build_nodes(&tokens, &mut i);
  render_nodes(&nodes, scope)
}

#[derive(Debug)]
enum Token {
  Text(String),
  OpenIf(String),
  OpenEach(String),
  Else,
  CloseIf,
  CloseEach,
  Expr(String),
}

fn tokenize_template(tpl: &str) -> Vec<Token> {
  let mut tokens = Vec::new();
  let mut rest = tpl;
  while let Some(rel) = rest.find("{{") {
    let before = &rest[..rel];
    if !before.is_empty() {
      tokens.push(Token::Text(before.to_string()));
    }
    let after = &rest[rel + 2..];
    match after.find("}}") {
      Some(close) => {
        let tag = after[..close].trim();
        tokens.push(if let Some(cond) = tag.strip_prefix("#if ") {
          Token::OpenIf(cond.trim().to_string())
        } else if let Some(key) = tag.strip_prefix("#each ") {
          Token::OpenEach(key.trim().to_string())
        } else if tag == "else" {
          Token::Else
        } else if tag == "/if" {
          Token::CloseIf
        } else if tag == "/each" {
          Token::CloseEach
        } else {
          Token::Expr(tag.to_string())
        });
        rest = &after[close + 2..];
      }
      None => {
        tokens.push(Token::Text(rest.to_string()));
        break;
      }
    }
  }
  if !rest.is_empty() {
    tokens.push(Token::Text(rest.to_string()));
  }
  tokens
}

#[derive(Debug)]
enum Node {
  Text(String),
  Expr(String),
  If {
    cond: String,
    then: Vec<Node>,
    else_: Vec<Node>,
  },
  Each {
    key: String,
    body: Vec<Node>,
  },
}

fn build_nodes(tokens: &[Token], i: &mut usize) -> Vec<Node> {
  let mut out = Vec::new();
  while *i < tokens.len() {
    match &tokens[*i] {
      Token::Text(t) => {
        out.push(Node::Text(t.clone()));
        *i += 1;
      }
      Token::Expr(e) => {
        out.push(Node::Expr(e.clone()));
        *i += 1;
      }
      Token::OpenIf(cond) => {
        *i += 1;
        let then = build_nodes(tokens, i);
        let mut else_ = Vec::new();
        if matches!(tokens.get(*i), Some(Token::Else)) {
          *i += 1;
          else_ = build_nodes(tokens, i);
        }
        if matches!(tokens.get(*i), Some(Token::CloseIf)) {
          *i += 1;
        }
        out.push(Node::If {
          cond: cond.clone(),
          then,
          else_,
        });
      }
      Token::OpenEach(key) => {
        *i += 1;
        let body = build_nodes(tokens, i);
        if matches!(tokens.get(*i), Some(Token::CloseEach)) {
          *i += 1;
        }
        out.push(Node::Each {
          key: key.clone(),
          body,
        });
      }
      _ => break,
    }
  }
  out
}

fn render_nodes(nodes: &[Node], scope: &Scope) -> String {
  let mut out = String::new();
  for node in nodes {
    match node {
      Node::Text(t) => out.push_str(t),
      Node::Expr(e) => out.push_str(&eval_expr(e, scope)),
      Node::If {
        cond,
        then,
        else_,
      } => {
        if truthy(lookup_path(cond, scope)) {
          out.push_str(&render_nodes(then, scope));
        } else {
          out.push_str(&render_nodes(else_, scope));
        }
      }
      Node::Each { key, body } => {
        if let Some(seq) = lookup_path(key, scope).and_then(|v| v.as_sequence()) {
          for item in seq {
            let mut sub = scope.to_vec();
            sub.push(item.clone());
            out.push_str(&render_nodes(body, &sub));
          }
        }
      }
    }
  }
  out
}

fn lookup_path<'a>(path: &str, scope: &'a Scope) -> Option<&'a serde_yaml::Value> {
  let path = path.trim();
  if path == "this" || path == "." {
    return scope.last();
  }
  let mut drops = 0usize;
  let mut name = path;
  while let Some(rest) = name.strip_prefix("../") {
    drops += 1;
    name = rest;
  }
  let idx = scope.len().checked_sub(drops + 1)?;
  let map = scope.get(idx)?.as_mapping()?;
  map.get(name)
}

fn truthy(v: Option<&serde_yaml::Value>) -> bool {
  match v {
    None => false,
    Some(serde_yaml::Value::String(s)) => !s.trim().is_empty(),
    Some(serde_yaml::Value::Sequence(seq)) => !seq.is_empty(),
    Some(serde_yaml::Value::Mapping(m)) => !m.is_empty(),
    Some(serde_yaml::Value::Null) => false,
    Some(other) => !stringify(other).is_empty(),
  }
}

fn stringify(v: &serde_yaml::Value) -> String {
  match v {
    serde_yaml::Value::String(s) => s.clone(),
    serde_yaml::Value::Number(n) => n.to_string(),
    serde_yaml::Value::Bool(b) => b.to_string(),
    serde_yaml::Value::Sequence(seq) => seq
      .iter()
      .map(stringify)
      .collect::<Vec<_>>()
      .join(", "),
    _ => String::new(),
  }
}

#[derive(Debug)]
enum Expr {
  Path(String),
  Lit(String),
  Call { name: String, args: Vec<Expr> },
}

#[derive(Debug)]
enum EToken {
  Name(String),
  Str(String),
  LParen,
  RParen,
  Comma,
}

fn tokenize_expr(s: &str) -> Vec<EToken> {
  let chars: Vec<char> = s.chars().collect();
  let mut toks = Vec::new();
  let mut i = 0usize;
  while i < chars.len() {
    let c = chars[i];
    if c.is_whitespace() {
      i += 1;
      continue;
    }
    match c {
      '(' => {
        toks.push(EToken::LParen);
        i += 1;
      }
      ')' => {
        toks.push(EToken::RParen);
        i += 1;
      }
      ',' => {
        toks.push(EToken::Comma);
        i += 1;
      }
      '"' => {
        i += 1;
        let mut lit = String::new();
        while i < chars.len() {
          let ch = chars[i];
          if ch == '\\' && i + 1 < chars.len() {
            lit.push(chars[i + 1]);
            i += 2;
          } else if ch == '"' {
            i += 1;
            break;
          } else {
            lit.push(ch);
            i += 1;
          }
        }
        toks.push(EToken::Str(lit));
      }
      _ => {
        let mut name = String::new();
        while i < chars.len()
          && (chars[i].is_alphanumeric()
            || matches!(chars[i], '_' | '-' | '.' | '/'))
        {
          name.push(chars[i]);
          i += 1;
        }
        toks.push(EToken::Name(name));
      }
    }
  }
  toks
}

fn parse_expr(s: &str) -> Option<Expr> {
  let toks = tokenize_expr(s);
  let mut i = 0usize;
  parse_expr_inner(&toks, &mut i)
}

fn parse_expr_inner(toks: &[EToken], i: &mut usize) -> Option<Expr> {
  match toks.get(*i)? {
    EToken::Str(l) => {
      *i += 1;
      Some(Expr::Lit(l.clone()))
    }
    EToken::Name(n) => {
      let name = n.clone();
      *i += 1;
      if matches!(toks.get(*i), Some(EToken::LParen)) {
        *i += 1;
        let mut args = Vec::new();
        if !matches!(toks.get(*i), Some(EToken::RParen)) {
          loop {
            args.push(parse_expr_inner(toks, i)?);
            match toks.get(*i) {
              Some(EToken::Comma) => *i += 1,
              Some(EToken::RParen) => {
                *i += 1;
                break;
              }
              _ => return None,
            }
          }
        } else {
          *i += 1;
        }
        Some(Expr::Call { name, args })
      } else {
        Some(Expr::Path(name))
      }
    }
    _ => None,
  }
}

fn eval_expr(s: &str, scope: &Scope) -> String {
  match parse_expr(s) {
    Some(e) => stringify(&eval_expr_value(&e, scope)),
    None => String::new(),
  }
}

fn eval_expr_value(e: &Expr, scope: &Scope) -> serde_yaml::Value {
  match e {
    Expr::Lit(l) => serde_yaml::Value::String(l.clone()),
    Expr::Path(p) => lookup_path(p, scope).cloned().unwrap_or(serde_yaml::Value::Null),
    Expr::Call { name, args } => {
      let vals: Vec<serde_yaml::Value> = args.iter().map(|a| eval_expr_value(a, scope)).collect();
      apply_call(name, &vals)
    }
  }
}

fn apply_call(name: &str, args: &[serde_yaml::Value]) -> serde_yaml::Value {
  let arg_str = |i: usize| -> String { args.get(i).map(stringify).unwrap_or_default() };
  match name {
    "join" => serde_yaml::Value::String(join_values(
      args.first().unwrap_or(&serde_yaml::Value::Null),
      "",
      "",
      &arg_str(1),
    )),
    "join_html" => {
      let sep = args.get(1).map(stringify).unwrap_or_else(|| ", ".to_string());
      let base = args.get(2).map(stringify).unwrap_or_else(|| "<b>".to_string());
      let close = args.get(3).map(stringify).unwrap_or_else(|| "</b>".to_string());
      serde_yaml::Value::String(join_values(
        args.first().unwrap_or(&serde_yaml::Value::Null),
        &base,
        &close,
        &sep,
      ))
    }
    "fallback" => {
      let a = args.get(0).unwrap_or(&serde_yaml::Value::Null);
      if !stringify(a).trim().is_empty() {
        a.clone()
      } else {
        args.get(1).cloned().unwrap_or(serde_yaml::Value::Null)
      }
    }
    "letters" => serde_yaml::Value::String(arg_str(0).chars().count().to_string()),
    "upper_first" => {
      let s = arg_str(0);
      serde_yaml::Value::String(
        s.chars()
          .next()
          .map(|f| f.to_uppercase().collect::<String>())
          .unwrap_or_default(),
      )
    }
    "underline" => serde_yaml::Value::String(underline_word(&arg_str(0), &arg_str(1))),
    "replace" => serde_yaml::Value::String(replace_word(&arg_str(0), &arg_str(1), &arg_str(2))),
    _ => serde_yaml::Value::Null,
  }
}

fn join_values(list: &serde_yaml::Value, base: &str, close: &str, sep: &str) -> String {
  let items: Vec<String> = match list {
    serde_yaml::Value::Sequence(seq) => seq.iter().map(stringify).collect(),
    other => vec![stringify(other)],
  };
  items
    .iter()
    .map(|s| format!("{}{}{}", base, s, close))
    .collect::<Vec<_>>()
    .join(sep)
}

/// Parse a component template file (front matter with `deck:` and `id:`, then body).
fn parse_template_file(path: &Path) -> Option<ComponentTemplate> {
  let content = fs::read_to_string(path).ok()?;
  let trimmed = content.trim_start();
  if !trimmed.starts_with("---") {
    return None;
  }
  let after_first = &trimmed[3..];
  let end = after_first.find("---")?;
  let matter = after_first[..end].trim();
  let body_start = 3 + end + 3;

  let mut deck = String::new();
  let mut id = String::new();
  let mut widget = String::new();
  for line in matter.lines() {
    let line = line.trim();
    if let Some(v) = line.strip_prefix("deck:") {
      deck = v.trim().to_string();
    } else if let Some(v) = line.strip_prefix("id:") {
      id = v.trim().trim_matches('"').to_string();
    } else if let Some(v) = line.strip_prefix("widget:") {
      widget = v.trim().to_string();
    }
  }

  let name = path
    .file_stem()
    .and_then(|s| s.to_str())
    .unwrap_or_default()
    .to_string();

  Some(ComponentTemplate {
    name,
    deck,
    id,
    widget,
    body: trimmed[body_start..].to_string(),
  })
}

/// Byte ranges of every whole-word, case-insensitive match of `word` in `text`.
/// Only char boundaries are considered, so slicing `text` with the returned
/// ranges can never panic and works with non-ASCII surrounding characters.
fn word_matches(text: &str, word: &str) -> Vec<(usize, usize)> {
  if word.is_empty() {
    return Vec::new();
  }
  let mut ranges = Vec::new();
  for (pos, _) in text.char_indices() {
    let rest = &text[pos..];
    let matches = match rest.get(..word.len()) {
      Some(sub) => sub.len() == word.len() && sub.eq_ignore_ascii_case(word),
      None => false,
    };
    if matches {
      let end = pos + word.len();
      let before_ok = pos == 0 || !text[..pos].chars().next_back().unwrap().is_alphanumeric();
      let after_ok = end == text.len() || !text[end..].chars().next().unwrap().is_alphanumeric();
      if before_ok && after_ok {
        ranges.push((pos, end));
      }
    }
  }
  ranges
}

fn underline_word(text: &str, word: &str) -> String {
  let mut out = String::with_capacity(text.len() + 8);
  let mut last = 0usize;
  for (s, e) in word_matches(text, word) {
    out.push_str(&text[last..s]);
    out.push_str("<u>");
    out.push_str(&text[s..e]);
    out.push_str("</u>");
    last = e;
  }
  out.push_str(&text[last..]);
  out
}

fn replace_word(text: &str, word: &str, replacement: &str) -> String {
  let mut out = String::with_capacity(text.len() + replacement.len());
  let mut last = 0usize;
  for (s, e) in word_matches(text, word) {
    out.push_str(&text[last..s]);
    out.push_str(replacement);
    last = e;
  }
  out.push_str(&text[last..]);
  out
}

/// Render a component template against one data record to a Card.
fn build_component_card(record: &serde_yaml::Value, tpl: &ComponentTemplate) -> Option<Card> {
  let scope = vec![record.clone()];
  let id = render_template(&tpl.id, &scope);
  if id.is_empty() {
    return None;
  }
  let body = render_template(&tpl.body, &scope);
  let (style, front, back, typed, answers, widget, extras) = split_sections(&body);
  if front.is_empty() && back.is_empty() {
    return None;
  }
  Some(Card {
    id,
    dependencies: Vec::new(),
    style,
    front: md_to_html(&front),
    back: md_to_html(&back),
    typed,
    answers,
    widget,
    widget_name: tpl.widget.clone(),
    extras,
    deck: tpl.deck.clone(),
    deleted: false,
  })
}

/// Validate one record against the deck's schema declaration.
fn validate_record(record: &serde_yaml::Value, spec: &RecordSpec, label: &str) -> Result<(), String> {
  let map = match record.as_mapping() {
    Some(m) => m,
    None => return Err(format!("Record '{}' is not a mapping", label)),
  };
  for field in &spec.required {
    let ok = match map.get(field.as_str()) {
      Some(serde_yaml::Value::String(s)) => !s.trim().is_empty(),
      Some(serde_yaml::Value::Sequence(seq)) => !seq.is_empty(),
      Some(_) => true,
      None => false,
    };
    if !ok {
      return Err(format!("Record '{}' is missing required field '{}'", label, field));
    }
  }
  for (name, field_type) in &spec.types {
    if let Some(value) = map.get(name.as_str()) {
      validate_field(value, field_type, &format!("{}.{}", label, name))?;
    }
  }
  Ok(())
}

fn validate_field(
  value: &serde_yaml::Value,
  field_type: &FieldType,
  label: &str,
) -> Result<(), String> {
  match field_type {
    FieldType::List => {
      if !value.is_sequence() {
        return Err(format!("Field '{}' should be a list", label));
      }
    }
    FieldType::ListOfRecords(spec) => {
      let seq = value
        .as_sequence()
        .ok_or_else(|| format!("Field '{}' should be a list", label))?;
      for (i, item) in seq.iter().enumerate() {
        let m = item
          .as_mapping()
          .ok_or_else(|| format!("Item {} of '{}' should be a record", i, label))?;
        for sub in &spec.list {
          if let Some(sv) = m.get(sub.name.as_str()) {
            match &sub.inner {
              None => {
                if !sv.is_string() {
                  return Err(format!("Field '{}.{}.{}' should be a string", label, i, sub.name));
                }
              }
              Some(inner) => validate_field(sv, inner, &format!("{}.{}.{}", label, i, sub.name))?,
            }
          }
        }
      }
    }
  }
  Ok(())
}

/// Load `cards/schema.yaml`, validate every data record dynamically, then fan
/// each record out to the component templates its `components` field requests.
/// Without a schema file the deck gets no component cards.
pub fn build_component_cards(
  cards_dir: &Path,
  seen: &mut HashSet<(String, String)>,
) -> Result<Vec<Card>, String> {
  let schema_path = {
    let yaml = cards_dir.join("schema.yaml");
    if yaml.is_file() {
      yaml
    } else {
      cards_dir.join("schema.yml")
    }
  };
  if !schema_path.is_file() {
    return Ok(Vec::new());
  }

  let content = fs::read_to_string(&schema_path)
    .map_err(|e| format!("Error reading '{}': {}", schema_path.display(), e))?;
  let schema: PipelineSchema = serde_yaml::from_str(&content)
    .map_err(|e| format!("Error parsing '{}': {}", schema_path.display(), e))?;

  let data_path = cards_dir.join(&schema.data.file);
  if !data_path.is_file() {
    return Err(format!(
      "Schema '{}' declares data file '{}' which is missing",
      schema_path.display(),
      schema.data.file
    ));
  }
  let content = fs::read_to_string(&data_path)
    .map_err(|e| format!("Error reading '{}': {}", data_path.display(), e))?;
  let root: serde_yaml::Value = serde_yaml::from_str(&content)
    .map_err(|e| format!("Error parsing '{}': {}", data_path.display(), e))?;
  let records = root
    .get(&schema.data.list)
    .and_then(|v| v.as_sequence())
    .ok_or_else(|| format!("'{}' has no list key '{}'", data_path.display(), schema.data.list))?;

  let types_dir = cards_dir.join(&schema.templates_dir);
  if !types_dir.is_dir() {
    return Err(format!(
      "Schema '{}' declares templates dir '{}' which is missing",
      schema_path.display(),
      schema.templates_dir
    ));
  }
  let mut templates: HashMap<String, ComponentTemplate> = HashMap::new();
  for entry in fs::read_dir(&types_dir).map_err(|e| format!("Error reading '{}': {}", types_dir.display(), e))? {
    let entry = entry.map_err(|e| e.to_string())?;
    let path = entry.path();
    if path.extension().and_then(|s| s.to_str()) == Some("html") {
      if let Some(tpl) = parse_template_file(&path) {
        templates.insert(tpl.name.clone(), tpl);
      }
    }
  }

  let mut cards = Vec::new();
  for (i, record) in records.iter().enumerate() {
    let label = {
      let guess = schema
        .record
        .required
        .first()
        .and_then(|f| record.as_mapping()?.get(f.as_str()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
      match guess {
        Some(s) if !s.is_empty() => s,
        _ => format!("#{}", i + 1),
      }
    };
    validate_record(record, &schema.record, &label)?;

    let components = record
      .get(&schema.data.components)
      .and_then(|v| v.as_sequence())
      .ok_or_else(|| format!("Record '{}' has no '{}' list", label, schema.data.components))?;

    for comp in components {
      let name = comp
        .as_str()
        .ok_or_else(|| format!("Component entry for '{}' must be a string", label))?;
      let tpl = match templates.get(name) {
        Some(t) => t,
        None => return Err(format!("Unknown component '{}' for '{}'", name, label)),
      };
      if let Some(card) = build_component_card(record, tpl) {
        if !seen.insert((card.deck.clone(), card.id.clone())) {
          return Err(format!("Duplicate card '{}::{}'", card.deck, card.id));
        }
        cards.push(card);
      }
    }
  }

  Ok(cards)
}

pub fn md_to_html(input: &str) -> String {
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
  let (id, dependencies, widget_name) = parse_front_matter(raw)?;

  let body = {
    let trimmed = raw.trim_start();
    let after_open = &trimmed[3..];
    let end = after_open.find("---").unwrap();
    &trimmed[3 + end + 3..]
  };

  let (style, front, back, typed, answers, widget, extras) = split_sections(body);

  if front.is_empty() && back.is_empty() {
    return None;
  }

  Some(Card {
    id,
    dependencies,
    style,
    front: md_to_html(&front),
    back: md_to_html(&back),
    typed,
    answers,
    widget,
    widget_name,
    extras,
    deck: String::new(),
    deleted: false,
  })
}

fn collect_md_files(dir: &Path, base: &Path, cards: &mut Vec<Card>, seen: &mut HashSet<(String, String)>) -> Result<(), String> {
  let entries = fs::read_dir(dir).map_err(|e| format!("Error reading directory: {}", e))?;

  for entry in entries.flatten() {
    let path = entry.path();

    if path.is_dir() {
      collect_md_files(&path, base, cards, seen)?;
    } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
      let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
      let is_deleted = file_name.ends_with(".del.md");

      let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(err) => {
          eprintln!("Error reading '{}': {}", path.display(), err);
          continue;
        }
      };

      match parse_card(&content) {
        Some(mut card) => {
          card.deleted = is_deleted;

          let rel = path.parent().unwrap().strip_prefix(base).unwrap_or(Path::new(""));
          if !rel.as_os_str().is_empty() {
            card.deck = rel.components()
              .map(|c| c.as_os_str().to_string_lossy())
              .collect::<Vec<_>>()
              .join("::");
          }

          if !seen.insert((card.deck.clone(), card.id.clone())) {
            let location = if card.deck.is_empty() { card.id.clone() } else { format!("{}::{}", card.deck, card.id) };
            return Err(format!("Duplicate card '{}'", location));
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

pub fn parse_dir(dir_path: &str) -> Result<Vec<Card>, String> {
  let dir = Path::new(dir_path);
  if !dir.is_dir() {
    return Err(format!("'{}' is not a directory", dir_path));
  }

  let mut cards = Vec::new();
  let mut seen = HashSet::new();

  collect_md_files(dir, dir, &mut cards, &mut seen)?;
  cards.extend(build_component_cards(dir, &mut seen)?);

  Ok(cards)
}

fn is_del_md(path: &Path) -> bool {
  path.file_name()
    .and_then(|n| n.to_str())
    .map(|n| n.ends_with(".del.md"))
    .unwrap_or(false)
}

fn find_card_file_by_id(dir: &Path, target: &str) -> Result<std::path::PathBuf, String> {
  let (target_deck, target_id) = if let Some(idx) = target.find("::") {
    (Some(&target[..idx]), &target[idx + 2..])
  } else {
    (None, target)
  };

  let mut found = None;

  fn walk(dir: &Path, target_deck: Option<&str>, target_id: &str, base: &Path, found: &mut Option<std::path::PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Error reading directory: {}", e))?;
    for entry in entries.flatten() {
      if found.is_some() { break; }
      let path = entry.path();
      if path.is_dir() {
        walk(&path, target_deck, target_id, base, found)?;
      } else if path.extension().and_then(|e| e.to_str()) == Some("md") && !is_del_md(&path) {
        if let Ok(content) = fs::read_to_string(&path) {
          if let Some((id, _, _)) = parse_front_matter(&content) {
            let rel = path.parent().unwrap().strip_prefix(base).unwrap_or(Path::new(""));
            let deck = if rel.as_os_str().is_empty() { String::new() } else {
              rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("::")
            };
            if id == target_id {
              if let Some(td) = target_deck {
                if deck == td || (td.is_empty() && deck.is_empty()) {
                  *found = Some(path);
                  break;
                }
              } else {
                *found = Some(path);
                break;
              }
            }
          }
        }
      }
    }
    Ok(())
  }

  walk(dir, target_deck, target_id, dir, &mut found)?;

  found.ok_or_else(|| format!("Card '{}' not found", target))
}

pub fn soft_delete(cards_dir: &str, target: &str) -> Result<String, String> {
  let path = Path::new(target);

  let file_path = if path.exists() && path.extension().and_then(|e| e.to_str()) == Some("md") {
    if is_del_md(path) {
      return Err(format!("Card '{}' is already deleted", target));
    }
    path.to_path_buf()
  } else {
    find_card_file_by_id(Path::new(cards_dir), target)?
  };

  let parent = file_path.parent().unwrap();
  let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
  let new_path = parent.join(format!("{}.del.md", stem));

  let mut content = fs::read_to_string(&file_path).map_err(|e| format!("Failed to read: {}", e))?;
  content.push_str("\n\n---\n\n> **This card has been deleted.**\n>\n> Edit this file to explain why.\n");
  fs::write(&new_path, &content).map_err(|e| format!("Failed to write: {}", e))?;
  fs::remove_file(&file_path).map_err(|e| format!("Failed to remove original: {}", e))?;
  Ok(format!("Deleted '{}'", file_path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_card(id: &str, deps: &[&str], typed: &str, front: &str, back: &str) -> Card {
    Card {
      id: id.to_string(),
      dependencies: deps.iter().map(|s| s.to_string()).collect(),
      style: String::new(),
      front: md_to_html(front),
      back: md_to_html(back),
      typed: typed.to_string(),
      answers: String::new(),
      widget: String::new(),
      widget_name: String::new(),
      extras: Vec::new(),
      deck: String::new(),
      deleted: false,
    }
  }

  #[test]
  fn parse_basic_card() {
    let input = "---\nid: gg\ndependencies: []\n---\n\n# Front\n\nUm\n\n---\n\n# Back\n\nHey";
    let card = parse_card(input).unwrap();
    assert_eq!(card, make_card("gg", &[], "", "Um", "Hey"));
  }

  #[test]
  fn parse_card_with_deps() {
    let input = "---\nid: baz\ndependencies: [gg, foo]\n---\n\n# Front\n\nQ\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert_eq!(card, make_card("baz", &["gg", "foo"], "", "Q", "A"));
  }

  #[test]
  fn parse_card_with_typed_answer() {
    let input = "---\nid: typed\ndependencies: []\n---\n\n# Front\n\nWhat word fits?\n\n---\n\n# Type\n\nsignificant\n\n---\n\n# Back\n\n**significant** — large and important";
    let card = parse_card(input).unwrap();
    assert_eq!(card.id, "typed");
    assert_eq!(card.typed, "significant");
    assert!(card.back.contains("<strong>significant</strong>"));
  }

  #[test]
  fn parse_card_with_widget_section() {
    let input = "---\nid: wid\ndependencies: []\n---\n\n# Front\n\nQ\n\n---\n\n# Widget\n\n<div class=\"type-box\" data-answers=\"{{answers}}\"></div>\n\n---\n\n# Type\n\nword1, word2\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert_eq!(card.typed, "word1, word2");
    assert!(card.widget.contains("data-answers=\"{{answers}}\""));
  }

  #[test]
  fn parse_card_with_widget_front_matter() {
    let input = "---\nid: wid2\ndependencies: []\nwidget: handwriting-canvas\n---\n\n# Front\n\nQ\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert_eq!(card.widget_name, "handwriting-canvas");
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
  fn parse_card_with_extra_section() {
    let input = "---\nid: write\ndependencies: []\n---\n\n# Front\n\nka\n\n---\n\n# Write\n\nカ,か\n\n---\n\n# Back\n\n**カ** — katakana \"ka\"";
    let card = parse_card(input).unwrap();
    assert_eq!(card.id, "write");
    assert_eq!(card.extras, vec![("Write".to_string(), "カ,か".to_string())]);
    assert_eq!(card.typed, "");
  }

  #[test]
  fn parse_card_multiple_extra_sections_in_order() {
    let input = "---\nid: multi\ndependencies: []\n---\n\n# Front\n\nQ\n\n---\n\n# Write\n\nカ\n\n---\n\n# Phonetic\n\n/ka/\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert_eq!(
      card.extras,
      vec![("Write".to_string(), "カ".to_string()), ("Phonetic".to_string(), "/ka/".to_string())]
    );
  }

  #[test]
  fn template_yaml_parses_fields() {
    let dir = std::env::temp_dir().join(format!("ankidaiku_tpl_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      dir.join("template.yaml"),
      "qfmt: |\n  {{Front}}\n  {{#Write}}box{{/Write}}\nafmt: '{{FrontSide}}<hr id=\"answer\">{{Back}}'\nfields:\n  - name: Write\n    plain: true\n  - name: Phonetic\n",
    )
    .unwrap();
    let tpl = load_deck_template(&dir).unwrap().unwrap();
    assert!(tpl.qfmt.unwrap().contains("{{#Write}}"));
    assert!(tpl.afmt.unwrap().contains("answer"));
    assert_eq!(tpl.fields.len(), 2);
    assert_eq!(tpl.fields[0].name, "Write");
    assert!(tpl.fields[0].plain);
    assert!(!tpl.fields[1].plain);
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn template_yaml_missing_is_none() {
    assert!(load_deck_template(Path::new("/nonexistent-dir")).unwrap().is_none());
  }

  #[test]
  fn duplicate_ids_in_same_deck_detected() {
    let mut seen = HashSet::new();
    assert!(seen.insert(("".to_string(), "dup".to_string())));
    assert!(!seen.insert(("".to_string(), "dup".to_string())));
  }

  #[test]
  fn same_id_in_different_decks_allowed() {
    let mut seen = HashSet::new();
    assert!(seen.insert(("Math".to_string(), "card-1".to_string())));
    assert!(seen.insert(("Science".to_string(), "card-1".to_string())));
  }

  fn record_achieve() -> serde_yaml::Value {
    serde_yaml::from_str(
      "word: achieve
ipa: /əːtʃiːv/
pos: verb
tip: a-chieve — the common 'ie' after c is the trap
spelling_hint: ''
definition: To successfully reach a desired aim or result by effort.
example: She worked hard to achieve her goals.
synonyms: [attain, accomplish, reach, realize, fulfil]
syn_pos: verb
syn_example: Achieve / attain / reach your goals — all are formal IELTS verbs.
syn_id: ''
syn_examples: []
components: [spelling, definitions, synonyms]",
    )
    .unwrap()
  }

  fn render_true_value(input: &str, tpl: &str) -> String {
    let record: serde_yaml::Value = serde_yaml::from_str(input).unwrap();
    let scope = vec![record];
    render_template(tpl, &scope)
  }

  #[test]
  fn render_template_substitutes_utf8_safely() {
    let out = render_true_value("word: liability\nemoji: —", "<div>{{word}}</div>\n<p>{{emoji}}</p>");
    assert_eq!(out, "<div>liability</div>\n<p>—</p>");
  }

  #[test]
  fn render_template_missing_prop_is_empty() {
    assert_eq!(render_template("a{{nothing}}b", &Vec::new()), "ab");
  }

  #[test]
  fn render_if_else() {
    let out = render_true_value("a: hi", "{{#if a}}Y{{else}}N{{/if}}");
    assert_eq!(out, "Y");
    let out = render_true_value("a: ''", "{{#if a}}Y{{else}}N{{/if}}");
    assert_eq!(out, "N");
  }

  #[test]
  fn render_each_with_parent_access() {
    let out = render_true_value(
      "word: achieve\nlist:\n  - {the: One}\n  - {the: Two}",
      "{{#each list}}{{the}}-{{../word}};{{/each}}",
    );
    assert_eq!(out, "One-achieve;Two-achieve;");
  }

  #[test]
  fn render_each_empty_list_is_empty() {
    let out = render_true_value("list: []", "A{{#each list}}x{{/each}}B");
    assert_eq!(out, "AB");
  }

  #[test]
  fn render_join_and_literal_arg() {
    let out = render_true_value("synonyms: [a, b, c]", "{{join(synonyms, \", \")}}");
    assert_eq!(out, "a, b, c");
  }

  #[test]
  fn render_join_html_wraps_items() {
    let out = render_true_value("synonyms: [a, b]", "{{join_html(synonyms, \" &middot; \")}}");
    assert_eq!(out, "<b>a</b> &middot; <b>b</b>");
  }

  #[test]
  fn render_fallback() {
    let out = render_true_value("x: ''\ny: real", "{{fallback(x, y)}}");
    assert_eq!(out, "real");
    let out = render_true_value("x: keep\ny: real", "{{fallback(x, y)}}");
    assert_eq!(out, "keep");
  }

  #[test]
  fn render_fallback_keeps_list_for_nested_join_html() {
    // `fallback` must preserve the list shape so a nested join_html still
    // wraps each item, rather than stringifying the list first.
    let out = render_true_value(
      "local: [a, b]\nother: [c]",
      "{{join_html(fallback(local, other), \" / \")}}",
    );
    assert_eq!(out, "<b>a</b> / <b>b</b>");
    let out = render_true_value(
      "local: []\nother: [c]",
      "{{join_html(fallback(local, other), \" / \")}}",
    );
    assert_eq!(out, "<b>c</b>");
  }

  #[test]
  fn render_letters_and_upper_first() {
    let out = render_true_value("word: achieve", "{{letters(word)}} {{upper_first(word)}}");
    assert_eq!(out, "7 A");
  }

  #[test]
  fn render_underline_and_replace() {
    let out = render_true_value("the: She worked hard to achieve her goals.\nword: achieve",
      "{{underline(the, word)}}");
    assert_eq!(out, "She worked hard to <u>achieve</u> her goals.");
    let out = render_true_value("the: I achieve my goals.\nword: achieve",
      "{{replace(the, word, \"reach\")}}");
    assert_eq!(out, "I reach my goals.");
  }

  #[test]
  fn render_list_path_stringifies() {
    let out = render_true_value("synonyms: [a, b]", "{{synonyms}}");
    assert_eq!(out, "a, b");
  }

  #[test]
  fn parse_card_with_answers() {
    let input = "---\nid: typed\ndependencies: []\n---\n\n# Front\n\nQ\n\n---\n\n# Type\n\nword\n\n---\n\n# Answers\n\nword, synonym\n\n---\n\n# Back\n\nA";
    let card = parse_card(input).unwrap();
    assert_eq!(card.typed, "word");
    assert_eq!(card.answers, "word, synonym");
  }

  #[test]
  fn component_renders_from_templates_and_matches_handwritten() {
    // Authoritative check: render via the schema-driven component engine and
    // compare to the hand-written .md cards for the same words. Requires the
    // deck repo, so it is skipped unless ANKI_DAIKU_DECK points at it.
    let deck = match std::env::var("ANKI_DAIKU_DECK") {
      Ok(d) => d,
      Err(_) => return,
    };
    let cards_dir = Path::new(&deck).join("cards");

    let schema_path = cards_dir.join("schema.yaml");
    if !schema_path.is_file() {
      return;
    }
    let schema: PipelineSchema =
      serde_yaml::from_str(&fs::read_to_string(&schema_path).unwrap()).unwrap();

    let mut templates = HashMap::new();
    let types_dir = cards_dir.join(&schema.templates_dir);
    for entry in fs::read_dir(&types_dir).unwrap().flatten() {
      let path = entry.path();
      if path.extension().and_then(|s| s.to_str()) == Some("html") {
        let t = parse_template_file(&path).unwrap();
        templates.insert(t.name.clone(), t);
      }
    }

    let data_path = cards_dir.join(&schema.data.file);
    if !data_path.is_file() {
      return;
    }
    let root: serde_yaml::Value =
      serde_yaml::from_str(&fs::read_to_string(&data_path).unwrap()).unwrap();
    let records = root.get(&schema.data.list).unwrap().as_sequence().unwrap();

    for record in records {
      let comps = record.get(&schema.data.components).unwrap().as_sequence().unwrap();
      for comp in comps {
        let comp = comp.as_str().unwrap();
        let tpl = templates.get(comp).unwrap();
        let rendered = build_component_card(record, tpl).unwrap();
        let word = record.get("word").unwrap().as_str().unwrap();

        // Build the path to the handwritten counterpart.
        let file = if comp == "synonyms" {
          format!("syn-{}.md", word)
        } else {
          format!("{}.md", word)
        };
        let path = cards_dir.join(&tpl.deck).join(&file);
        if !path.is_file() {
          continue; // handwritten master may have been removed
        }
        let handwritten_raw = fs::read_to_string(&path).unwrap();
        let mut handwritten = parse_card(&handwritten_raw).unwrap();
        handwritten.deck = tpl.deck.clone();

        assert_eq!(
          (rendered.id.clone(), rendered.deck.clone(), rendered.style.clone(), rendered.front.clone(), rendered.back.clone(), rendered.typed.clone(), rendered.extras.clone()),
          (handwritten.id, handwritten.deck, handwritten.style, handwritten.front, handwritten.back, handwritten.typed, handwritten.extras),
          "component mismatch for {} -> {}",
          word,
          comp
        );
      }
    }
  }

  #[test]
  fn component_widget_section_kept_after_render() {
    let record = record_achieve();
    let tpl = ComponentTemplate {
      name: "spelling".into(),
      deck: "Spelling".into(),
      id: "{{word}}".into(),
      widget: String::new(),
      body: "# Front\n\nQ\n\n---\n\n# Widget\n\n<div data-word=\"{{word}}\"></div>\n\n---\n\n# Type\n\n{{word}}\n\n---\n\n# Back\n\nA".into(),
    };
    let card = build_component_card(&record, &tpl).unwrap();
    assert!(card.widget.contains("data-word=\"achieve\""));
  }

  #[test]
  fn component_answers_section_kept_after_render() {
    let record =
      serde_yaml::from_str::<serde_yaml::Value>("word: achieve\nsynonyms: [attain, reach]").unwrap();
    let tpl = ComponentTemplate {
      name: "synonyms".into(),
      deck: "Synonyms".into(),
      id: "{{word}}".into(),
      widget: String::new(),
      body: "# Front\n\nQ\n\n---\n\n# Type\n\n{{join(synonyms, \", \")}}\n\n---\n\n# Answers\n\n{{join(synonyms, \" / \")}}\n\n---\n\n# Back\n\nA".into(),
    };
    let card = build_component_card(&record, &tpl).unwrap();
    assert_eq!(card.typed, "attain, reach");
    assert_eq!(card.answers, "attain / reach");
  }

  #[test]
  fn component_custom_spelling_hint_used() {
    let record = serde_yaml::from_str::<serde_yaml::Value>(
      "word: achieve\npos: verb\nspelling_hint: custom hint here",
    )
    .unwrap();
    // Build a minimal spelling template inline (no filesystem dependency).
    let tpl = ComponentTemplate {
      name: "spelling".into(),
      deck: "Spelling".into(),
      id: "{{word}}".into(),
      widget: String::new(),
      body: "# Front\n\n<div class=\"hint\">{{#if spelling_hint}}{{spelling_hint}}{{else}}{{letters(word)}} letters &middot; starts with <b>{{upper_first(word)}}</b> &middot; {{pos}}{{/if}}</div>\n\n---\n\n# Type\n\n{{word}}\n\n---\n\n# Back\n\nx".into(),
    };
    let card = build_component_card(&record, &tpl).unwrap();
    assert!(card.front.contains("custom hint here"));
  }

  #[test]
  fn component_default_spelling_hint_derived() {
    let record =
      serde_yaml::from_str::<serde_yaml::Value>("word: achieve\npos: verb\nspelling_hint: ''").unwrap();
    let tpl = ComponentTemplate {
      name: "spelling".into(),
      deck: "Spelling".into(),
      id: "{{word}}".into(),
      widget: String::new(),
      body: "# Front\n\n<div class=\"hint\">{{#if spelling_hint}}{{spelling_hint}}{{else}}{{letters(word)}} letters &middot; starts with <b>{{upper_first(word)}}</b> &middot; {{pos}}{{/if}}</div>\n\n---\n\n# Type\n\n{{word}}\n\n---\n\n# Back\n\nx".into(),
    };
    let card = build_component_card(&record, &tpl).unwrap();
    assert!(card.front.contains("7 letters &middot; starts with <b>A</b> &middot; verb"));
  }

  #[test]
  fn schema_driven_pipeline() {
    let dir = std::env::temp_dir().join(format!("ankidaiku_schema_{}", std::process::id()));
    fs::create_dir_all(dir.join("types")).unwrap();
    fs::write(
      dir.join("schema.yaml"),
      "data: {file: data.yaml, list: words, components: components}\nrecord:\n  required: [word]\n  types:\n    synonyms: list\n",
    )
    .unwrap();
    fs::write(
      dir.join("data.yaml"),
      "words:\n  - {word: one, synonyms: [a, b], components: [spelling]}\n  - {word: two, synonyms: [c], components: [spelling]}\n",
    )
    .unwrap();
    fs::write(
      dir.join("types").join("spelling.html"),
      "---\ndeck: Spelling\nid: \"{{word}}\"\n---\n\n# Front\n\n{{word}}\n\n---\n\n# Type\n\n{{join(synonyms, \", \")}}\n\n---\n\n# Back\n\nx",
    )
    .unwrap();
    let mut seen = HashSet::new();
    let cards = build_component_cards(&dir, &mut seen).unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].id, "one");
    assert_eq!(cards[0].typed, "a, b");
    assert_eq!(cards[1].id, "two");
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn schema_validation_missing_required() {
    let dir = std::env::temp_dir().join(format!("ankidaiku_schema_req_{}", std::process::id()));
    fs::create_dir_all(dir.join("types")).unwrap();
    fs::write(
      dir.join("schema.yaml"),
      "data: {file: data.yaml, list: words, components: components}\nrecord:\n  required: [word]\n",
    )
    .unwrap();
    fs::write(dir.join("data.yaml"), "words:\n  - {word: '' }\n").unwrap();
    fs::write(
      dir.join("types").join("spelling.html"),
      "---\ndeck: D\nid: \"{{word}}\"\n---\n\n# Front\n\nQ\n\n---\n\n# Back\n\nA",
    )
    .unwrap();
    let mut seen = HashSet::new();
    let err = build_component_cards(&dir, &mut seen).unwrap_err();
    assert!(err.contains("missing required field 'word'"), "{}", err);
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn schema_unknown_component_error() {
    let dir = std::env::temp_dir().join(format!("ankidaiku_schema_unk_{}", std::process::id()));
    fs::create_dir_all(dir.join("types")).unwrap();
    fs::write(
      dir.join("schema.yaml"),
      "data: {file: data.yaml, list: words, components: components}\n",
    )
    .unwrap();
    fs::write(dir.join("data.yaml"), "words:\n  - {word: one, components: [nope]}\n").unwrap();
    let mut seen = HashSet::new();
    let err = build_component_cards(&dir, &mut seen).unwrap_err();
    assert!(err.contains("Unknown component 'nope'"), "{}", err);
    fs::remove_dir_all(&dir).unwrap();
  }
}
