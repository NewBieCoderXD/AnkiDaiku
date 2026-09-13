# AnkiDaiku

Build Anki `.apkg` packages from markdown cards.

## Card format

Cards live in `cards/` (or a custom directory). Each card is a `.md` file:

```md
---
id: my-card
dependencies: [prereq-card]
---

# Front

What is the capital of France?

---

# Back

Paris
```

Front matter supports:
- `id` (required) — unique card identifier within its sub-deck
- `dependencies` (optional) — space-separated card IDs for scheduling order
- `widget` (optional) — use `cards/widgets/<name>.html` as this card's answer box

Sections are separated by `---`:
- `# Style` (optional) — injected as `<style>` in front/back HTML
- `# Front` — shown on the question side
- `# Back` — shown on the answer side
- `# Type` (optional) — typed-answer answers, separated by commas or semicolons.
  Accepted answers are matched against **any** of them (case/space-insensitive),
  so `significant, crucial` accepts either. Wrong entries report which tokens
  were recognized and a "Show answers" button reveals the accepted list. Cards
  without a `# Type` section are plain front/back recall.
- `# Widget` (optional) — inline HTML/JS for *this* card's answer box; see
  Widgets below. `{{answers}}` is replaced with the escaped `# Type` list,
  or the `# Answers` section when one is present (see "Answers section").
- `# Answers` (optional) — overrides what the widget's `{{answers}}` sees; the
  accepted-answer matching itself still uses `# Type`.

### Widgets

Two templating levels work together:

- **Type** — the Anki-native notetype template (QFMT/AFMT). One copy per
  notetype, shared by every card. The built-in shell is
  `{{Front}}\n{{#Type}}{{Widget}}{{/Type}}`; decks can fully replace it via a
  `template.yaml` qfmt/afmt (see below).
- **Widget** — per-card interaction HTML/JS, built at build time and stored in
  the note's `Widget` field. Because it is field data it updates reliably on
  import (GUID-matched), unlike note-type template text.

Widget resolution, most specific first:

1. An inline `# Widget` section on the card (or on its component type in
   `cards/types/`, where `{{prop}}` placeholders are rendered first)
2. `cards/widgets/<name>.html` selected via the `widget:` front-matter key on
   the card or component type
3. `cards/widgets/default.html` (deck-wide default)
4. the built-in multi-answer widget

Cards without `# Type` answers get no widget. Widget references to media are
scanned and renamed like other fields.

### Sub-decks

Directory nesting creates sub-decks. `cards/Math/calculus.md` creates `my-deck::Math`.

IDs are scoped per sub-deck. The same `id` can appear in different sub-decks — each is treated as a separate card with its own GUID (derived from `deck::id`).

### Shared CSS

Create `shared.css` in the repo root, or configure via `ankidaiku.config.json`:

```json
{
  "shared_css": "path/to/theme.css"
}
```

The CSS is merged into the global Anki notetype CSS.

### Custom deck templates

By default every deck uses the built-in notetype (fields `Front`, `Back`,
`Type`) with the typed-answer box. Decks that need their own layout — e.g. a
handwriting canvas, cloze-style front, extra fields, or rich media — can ship
a `template.yaml` in the cards directory:

```yaml
qfmt: |
  {{Front}}
  {{#Write}}
  ...canvas, buttons, and grading script...
  {{/Write}}
afmt: '{{FrontSide}}<hr id="answer">{{Back}}'
css: |
  .write-canvas { border: 2px solid #333; }
fields:
  - name: Write
    plain: true
```

- `qfmt` / `afmt` (optional) — full Anki template strings replacing the
  built-in ones. `{{#Field}}` conditionals only render where a note has that
  field filled.
- `fields` (optional) — extra notetype fields beyond `Front`, `Back`, `Type`.
  When present, they become selectable sections in the card markdown:
  ```md
  # Front

  ka

  ---

  # Write

  カ,か

  ---

  # Back

  **カ** — katakana "ka"
  ```
  Any other `# Header` in a card (e.g. `# Write`, `# Phonetic`) is stored as
  an extra field (raw text, like `# Type`). Cards using a header that is not
  declared in `fields` are a build error.
- `css` (optional) — extra CSS merged after `shared.css` into the notetype.

Media referenced by `src="..."` in a custom template is embedded like any
other media; use flat filenames there (e.g. `src="_lib.js"`), since template
paths are not rewritten to basenames. Without a `template.yaml` the deck
behaves exactly as before.

## Component cards (schema-driven)

Data-driven decks (one data record → several card types) are built from a
schema file instead of hardcoding record shapes in the builder. When
`cards/schema.yaml` (or `schema.yml`) exists, it is auto-detected:

```yaml
data: { file: words.yaml, list: words, components: components }
templates_dir: types
record:
  required: [word, ipa, definition]
  types:
    synonyms: list
    syn_examples:
      list:
        - name: the
        - name: syns
          type: list
```

- `data` — which YAML/JSON/TSV file holds the records, and which top-level
  keys name the plain records (`list`) and the component records
  (`components`; each entry links back to a list record via `link`).
- `templates_dir` — where the `types/*.html` component templates live.
- `record.required` — missing / empty (or missing-component) fields fail the
  build. Unknown fields are allowed; unlisted optional fields default to empty.
- `record.types` — schema-enforced types: `list` (a list of plain scalars) or
  `list of records` (`list: [ { name, type } ... ]`).

Each component template is a normal card markdown file (`---` front matter,
`# Front` / `# Back` / `# Type` / `# Widget` sections) with `{{prop}}`
placeholders that are rendered from the linked data record. The template's
front matter can also use `{{prop}}`, e.g. `id: "{{word}}"`. Any card-level
`# Widget` / `widget:` and `# Type` rules apply; `# Type` on a component can
be `{{join(props, ", ")}}` and `# Widget` can show `{{answers}}`.

### Template expressions

Component templates support a small fixed set of expressions (no arbitrary
code):

- **Blocks** — `{{#if prop}}…{{else}}…{{/if}}` (empty/`0`/`false` is falsey)
  and `{{#each prop}}…{{/each}}` (empty runs zero times).
- **Paths** — `{{word}}`, and `../word` to step out of the current `#each`
  scope back to the record.
- **Calls** — `join(list, ", ")`, `join_html(list, " / ")` (each item
  `<b>…</b>`-wrapped), `fallback(a, b)` (keeps list shape), `letters(word)`,
  `upper_first(word)`, `underline(example, word)` (wraps whole-word matches
  in `<u>`), `replace(example, word, new)`. Arguments may be paths, string
  literals (`"…"`), or nested calls.
- Strings and lists stringify to plain text; missing props are empty and lists
  join with `", "`.

Because all formatting lives in the templates, the phone-app answers follow
the same pipeline as always — `# Type` supplies the accepted list.

### Answers section

`# Answers` is a build-time parameter for the widget's `{{answers}}`,
independent of `# Type` (which decides the accepted-answers matching). Where a
widget wants cleaner values (e.g. a comma list) than what `# Type` must show,
supply both:

```md
# Type

{{join(synonyms, ", ")}}

# Answers

{{join_html(synonyms, ", ")}}
```

When `# Answers` is empty or absent, `{{answers}}` falls back to the `# Type`
list — which is exactly the pre-existing behaviour when templates only use
`# Type`.

## Updating an existing collection

Re-importing a rebuilt `.apkg` keeps study progress: notes are matched by
stable GUID so fields are updated in place and scheduling is untouched. This
covers everything stored in *fields* — front/back content, `# Type` answers,
widgets, and extra fields.

The **Anki-native notetype** (fields list, QFMT/AFMT, CSS) is different: it is
a schema shared by the whole notetype, and Anki only applies template/schema
changes to a same-named existing notetype if you ask it to. Import into an
existing collection (Anki 23.10+) with:

- **"Update note types"** enabled (so template text and styling are imported),
- **"Merge note types"** enabled (so field/template additions — a schema
  change — are merged rather than left as a conflicting note type).

Without "Merge note types", schema changes (e.g. adding the `Widget` field) are
treated as a modified note type and existing notes may stop updating. Template
text and styling that are *not* schema changes still import whenever "Update
note types" allows them. Older Anki (before 23.10) and AnkiDroid cannot merge
schemas on import at all — import through a recent desktop Anki first, then
sync. After the one-off merge, the schema is stable and later builds update
purely via fields.

## Media files

Cards can reference images, audio, or video. Place media files in the `media/` directory:

```
media/
  cat.png               # referenced as ![cat](cat.png)
  lesson1.mp3           # referenced as <audio src="lesson1.mp3">
```

Media paths are resolved relative to this directory only — media lives separate from cards so it can be shared across decks.

The media directory defaults to `media/` alongside `cards/`. Configure via `ankidaiku.config.json`:

```json
{
  "media_dir": "assets/media"
}
```

**Collision rules:**
- Different HTML paths (`pic.png` vs `assets/pic.png`) resolving to the same file are merged into one media entry
- Different files with the same basename trigger an error

Media files are bundled inside the `.apkg` with integer IDs. Anki maps filenames automatically.

## Soft delete

```
ankidaiku delete my-card
ankidaiku delete Math::intro
ankidaiku delete ./cards/my-card.md
```

This renames `my-card.md` → `my-card.del.md` and appends a placeholder explanation:

> **This card has been deleted.**
>
> Edit this file to explain why.

Cards with the same ID in different sub-decks can be disambiguated with `deck::id` format.

The `.del.md` file is always included in the build — it shows whatever you write in the front/back. Edit the file to explain why the card was removed:

```md
---
id: my-card
dependencies: []
---

# Front

This card is no longer maintained

---

# Back

See the updated version at cards/Science/updated-card.md
```

For new Anki users, the card appears as-is in the apkg. For existing users, the same `id` + `deck` produces the same GUID, so Anki updates the note fields — they see your explanation.

## Configuration

All settings are optional:

```json
{
  "cards_dir": "my-cards",
  "shared_css": "assets/theme.css"
}
```

## CLI

```
ankidaiku build <dir>         Build .apkg from cards in <dir>
  -o, --output <path>         Output path (default: <dir>/dist/output.apkg)
  -C, --cards-dir <dir>       Cards directory (default: <dir>/cards/)
  --config <path>             Config file path

ankidaiku delete <target>     Soft-delete a card by ID (or deck::id) or file path
  -C, --cards-dir <dir>       Cards directory
  --config <path>             Config file path

```

The deck name comes from `package.json` → `name` in the repo root, falling back to `AnkiDeck`.
