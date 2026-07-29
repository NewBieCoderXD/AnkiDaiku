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

Sections are separated by `---`:
- `# Style` (optional) — injected as `<style>` in front/back HTML
- `# Front` — shown on the question side
- `# Back` — shown on the answer side

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
