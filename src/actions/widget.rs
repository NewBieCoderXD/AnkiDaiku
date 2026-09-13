use std::fs;
use std::path::Path;

use super::build::Card;

/// Default multi-answer widget: an input box, a Check button, a Show-answers
/// button, and a small grading script. Accepted answers (from `# Type`, or the
/// `# Answers` section that overrides it) can be
/// referenced as `{{answers}}`; the build substitutes the card's answer list
/// (HTML-escaped) so it is safe inside a quoted attribute.
const DEFAULT_WIDGET: &str = r##"<div class="type-box" id="td-wrap" data-answers="{{answers}}">
  <input id="td-input" class="typed-input" type="text"
         autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false"
         placeholder="Type your answer">
  <button id="td-btn" class="typed-btn" type="button">Check</button>
  <button id="td-show" class="typed-show" type="button">Show answers</button>
  <div id="td-msg" class="td-msg"></div>
</div>
<script>
(function() {
  var wrap = document.getElementById("td-wrap");
  if (!wrap || wrap.getAttribute("data-bound")) return;
  wrap.setAttribute("data-bound", "1");
  wrap.setAttribute("data-tries", "0");
  var answers = (wrap.getAttribute("data-answers") || "").split(/[,;]/).map(function(s) { return s.trim().toLowerCase(); }).filter(Boolean);
  var input = document.getElementById("td-input");
  var msg = document.getElementById("td-msg");
  function setMsg(html, cls) {
    msg.innerHTML = html;
    msg.className = "td-msg " + cls;
  }
  function check() {
    var given = (input.value || "").trim().toLowerCase();
    if (!given) { setMsg("Type an answer first.", "td-hint"); input.focus(); return; }
    var parts = given.split(/\s*[,;]\s*/).filter(Boolean);
    var isFull = answers.indexOf(given) >= 0;
    var matched = parts.filter(function(p) { return answers.indexOf(p) >= 0; });
    var ok = isFull || (matched.length > 0 && matched.length === parts.length);
    if (ok) {
      let label;
      if (parts.length!==answers.length){
        let remaining = answers.length - matched.length;
        setMsg(matched.length + " are correct. " + remaining + " more remaining.")
      } else {
        label = "All are correct";
      }
      setMsg(label + " ✓", "typeGood");
      input.className = "typed-input typeGood";
      return;
    }
    var tries = parseInt(wrap.getAttribute("data-tries") || "0", 10) + 1;
    wrap.setAttribute("data-tries", String(tries));
    input.className = "typed-input typeBad";
    if (matched.length > 0 && matched.length < parts.length) {
      var wrong = parts.filter(function(p) { return answers.indexOf(p) < 0; });
      var remaining = answers.length - matched.length;
      setMsg(matched.length + " are correct. " + remaining + " more remaining.\n unrecognized" + wrong.join(", ") + ".", "typeBad");
    } else if (matched.length === 0 && tries >= 3) {
      setMsg("Not quite. Accepted answers: " + answers.join(", ") + ".", "typeBad");
    } else if (matched.length === 0) {
      setMsg("None of those are in the answer list. Try again (" + tries + "/3) or tap Show answers.", "typeBad");
    } else {
      setMsg("Try again.", "typeBad");
    }
  }
  function reveal() {
    setMsg("Accepted answers: " + answers.join(", ") + ".", "td-hint");
  }
  input.addEventListener("keydown", function(e) { if (e.key === "Enter") check(); });
  document.getElementById("td-btn").addEventListener("click", check);
  document.getElementById("td-show").addEventListener("click", reveal);
  try {
    // Pre-focus the field so a visible keyboard opens on mobile where the
    // "Type answer into the card" setting is enabled.
    input.focus({ preventScroll: true });
  } catch (e) {}
})();
</script>"##;

/// Escape a value for safe inclusion in an HTML attribute value.
pub fn escape_attr(s: &str) -> String {
  s.replace('&', "&amp;")
    .replace('<', "&lt;")
    .replace('>', "&gt;")
    .replace('"', "&quot;")
}

fn substitute_answers(fragment: &str, typed: &str) -> String {
  fragment.replace("{{answers}}", &escape_attr(typed))
}

/// Resolve the final widget HTML for a card.
///
/// Most specific source wins:
///   1. an inline `# Widget` section on the card (or its component type),
///   2. a named widget file (`cards/widgets/<name>.html`) referenced via the
///      `widget:` front-matter key on the card or component type,
///   3. the deck-wide `cards/widgets/default.html` file,
///   4. the built-in multi-answer widget.
///
/// The `{{answers}}` parameter comes from the card's `# Answers` section when
/// present, otherwise from its `# Type` answers. Cards without `# Type` answers
/// get no widget.
pub fn resolve_widget(card: &Card, cards_dir: &Path) -> Result<String, String> {
  let typed = card.typed.trim().to_string();
  if typed.is_empty() {
    if !card.widget.is_empty() {
      eprintln!(
        "Warning: card '{}' defines a widget but has no '# Type' answers; ignoring it.",
        card.id
      );
    }
    return Ok(String::new());
  }

  let answers = {
    let answers = card.answers.trim();
    if answers.is_empty() {
      typed.as_str()
    } else {
      answers
    }
  };

  let fragment = if !card.widget.is_empty() {
    card.widget.clone()
  } else if !card.widget_name.is_empty() {
    let path = cards_dir.join("widgets").join(format!("{}.html", card.widget_name));
    if !path.is_file() {
      return Err(format!(
        "Widget '{}' for card '{}' not found in '{}'",
        card.widget_name,
        card.id,
        path.display()
      ));
    }
    fs::read_to_string(&path).map_err(|e| format!("Error reading widget '{}': {}", path.display(), e))?
  } else {
    let path = cards_dir.join("widgets").join("default.html");
    if path.is_file() {
      fs::read_to_string(&path).map_err(|e| format!("Error reading widget '{}': {}", path.display(), e))?
    } else {
      DEFAULT_WIDGET.to_string()
    }
  };

  Ok(substitute_answers(&fragment, answers))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn card(typed: &str, widget: &str, widget_name: &str) -> Card {
    Card {
      id: "test-card".into(),
      dependencies: Vec::new(),
      style: String::new(),
      front: String::new(),
      back: String::new(),
      typed: typed.into(),
      answers: String::new(),
      widget: widget.into(),
      widget_name: widget_name.into(),
      extras: Vec::new(),
      deck: String::new(),
      deleted: false,
    }
  }

  fn tmp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
      "ankidaiku_widget_{}_{}",
      std::process::id(),
      name
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn builtin_used_and_answers_escaped() {
    let c = card("foo, bar", "", "");
    let dir = tmp_dir("builtin");
    let out = resolve_widget(&c, &dir).unwrap();
    assert!(out.contains("data-answers=\"foo, bar\""));
    assert!(out.contains("td-wrap"));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn answers_attribute_escaped() {
    let c = card("a &amp; \"b\"", "", "");
    let dir = tmp_dir("escape");
    let out = resolve_widget(&c, &dir).unwrap();
    // The raw typed value is HTML-escaped when substituted into the attribute.
    assert!(out.contains("a &amp;amp; &quot;b&quot;"));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn inline_widget_wins() {
    let c = card("word", "<div class=\"mine\">custom</div>", "named");
    let dir = tmp_dir("inline");
    fs::create_dir_all(dir.join("widgets")).unwrap();
    fs::write(dir.join("widgets").join("default.html"), "DEFAULT").unwrap();
    let out = resolve_widget(&c, &dir).unwrap();
    assert_eq!(out, "<div class=\"mine\">custom</div>");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn named_widget_file_used_when_no_inline() {
    let c = card("word", "", "checker");
    let dir = tmp_dir("named");
    fs::create_dir_all(dir.join("widgets")).unwrap();
    fs::write(dir.join("widgets").join("checker.html"), "<p>{{answers}}</p>").unwrap();
    let out = resolve_widget(&c, &dir).unwrap();
    assert_eq!(out, "<p>word</p>");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn default_file_beats_builtin() {
    let c = card("word", "", "");
    let dir = tmp_dir("deffile");
    fs::create_dir_all(dir.join("widgets")).unwrap();
    fs::write(dir.join("widgets").join("default.html"), "D-{{answers}}").unwrap();
    let out = resolve_widget(&c, &dir).unwrap();
    assert_eq!(out, "D-word");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn answers_section_overrides_typed() {
    let mut c = card("word", "", "");
    c.answers = "extra, alternative".into();
    let dir = tmp_dir("answers");
    let out = resolve_widget(&c, &dir).unwrap();
    assert!(out.contains("data-answers=\"extra, alternative\""));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn no_answers_means_no_widget() {
    let c = card("", "", "");
    let dir = tmp_dir("noans");
    assert_eq!(resolve_widget(&c, &dir).unwrap(), "");
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn missing_named_widget_is_an_error() {
    let c = card("word", "", "nope");
    let dir = tmp_dir("missing");
    assert!(resolve_widget(&c, &dir).is_err());
    let _ = fs::remove_dir_all(&dir);
  }
}