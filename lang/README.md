# Translations of NVForever's own words

NVForever writes some words itself: the settings page, a few messages, the terminal
hacking log. The original game's text is not copied (LEGAL.md). The game's own text
(items, dialogue, quests) comes from the plugins of your install, so a localized install
already shows it in its language. Only NVForever's own words need a translation, and
this folder holds them, one file per language (#1443). Shipped (#1948): `de.toml`
(Deutsch), `es.toml` (Español), `fr.toml` (Français), `it.toml` (Italiano), `ru.toml`
(Русский) and `zh.toml` (简体中文).

## A language file

`<id>.toml`, UTF-8, for example `de.toml`:

```toml
name = "Deutsch"

[words]
"Language" = "Sprache"
"Accepted." = "Angenommen."
"{n} of {len} right" = "{n} von {len} richtig"
```

- `name` is how the settings page lists the language, in the language itself.
- Each line under `[words]` is the English text exactly as NVForever writes it, then
  your translation. A word with no line, or an empty one, stays English.
- `{n}`, `{len}`, `{needed}` and the like are filled in by NVForever. Keep them, in any
  order your language needs.
- Write your own words. Don't copy the original game's strings into a file
  (LEGAL.md).
- The settings page's own words are its tabs and categories (`Gameplay`, `Saving`, …),
  the Tweaks tab's groups and categories, `Search`, `Changed`, `Keyboard search`,
  `Previous Category`, `Next Category` and what an empty list says (`No settings here.`, `No results.`, `No changed settings.`). The
  search finds a row by its words in the language shown and in English, and by its INI
  key, so a translation never hides a row from it. A word with no line reads in English, in
  Title Case on the page. The menus' sharp letters (Display, Interface: Menu font) have
  Latin and Cyrillic whatever the install's code page; the game's own font has only its
  install's letters, so for a language it cannot write the menus are drawn sharp. `Stands for: {rows}.` begins the note
  of a row listed for others (its `{rows}` the rows it stands for).

## Where NVForever looks

1. This `lang` folder beside `nvforever.exe` (the translations that come with NVForever).
2. `lang` in NVForever's home (`Documents\My Games\NVForever\lang`), for your own.
3. The folder `NVF_LANG_DIR` names, if set.

When two folders have the same id, the first one wins. The language is chosen on the
settings page (Display, Interface: Language) and acts at once. It is kept as `language=<id>` in
`NVForever.ini`.

Until one is chosen there, the start picks the language Steam installed the game in, read
from Steam's manifest beside the game's folder (`appmanifest_22380.acf`, `UserConfig`
`language`; #1443): German, French, Italian, Spanish and Russian choose `de`, `fr`, `it`,
`es` and `ru` when that file was found and the game's text is read in that language's code
page. Outside a Steam library it picks the language of the game's text when its code page
tells it: Windows-1251 (a Russian install) chooses `ru` (#1948). Windows-1250 and 1252 are
each several languages, so the text alone chooses none and English stays. The start's log
says what it chose. The code page itself comes from `[Text] CodePage` when set, else from
the plugins' text (Cyrillic), else from Steam's language (1250 for Polish, Czech and
Hungarian), else 1252.

## Checked

`cargo test --workspace` reads every file here: each must parse, each English line must
still be a text NVForever writes (found in the sources), and each translation must keep
its line's `{places}`. A line whose English is changed in the code fails the check until
the file is updated too. And each text the code hands to `tr("…")` or `trf("…")` must
have a line in every file shipped here (#3146), so a new text cannot be left English by
forgetting them.

What goes through these files today: the main menu and the pause menu (their entries,
Back, Defaults, the quit and keep questions), the settings page (its tabs, rows, notes,
ON / OFF and the names of the choices in its lists and on its choice meters: Off, Auto,
Amber, Very Hard …), the hacking log and the terminal refusals (#2095), lockpicking and the casino games
(blackjack, roulette, slots: their buttons, rows, refusals and results, #3146; the
chips' and casinos' names are the install's), Caravan (its buttons, rows, refusals and
results), the key and mouse button names in the help boxes, the poison, VATS, death,
pickpocket and save notices (#3146), and the Quick Loot list (its keys' words, column heads and `Empty`; the items' and containers' names are the install's), and the corner and banner notices NVForever words itself (`Quest added`, `Quest completed`, `Quest failed`, `You have discovered`, `COMPLETED: …`, `Discovered: …`, `… added`, `Map marker added`, #3399; the quests', places' and items' names are the install's). A choice's
name with no line (a resolution, a display's name, a language's own name) shows as it
is. A note that names a choice names it as the page shows it, so a translated choice
name is translated in the notes too.

The ammo card's fallback labels also use these files when an AMEF has no FULL name:
`Damage`, `Damage resist`, `Damage threshold`, `Spread`, `Weapon wear`, `Fatigue`,
`Ammo effect` [design, #2502; `engine/nvf-play/src/pipboy/items.rs`]. An effect's own
FULL remains the load order's text [design, #2502].

## Simplified Chinese and fonts

Choose **简体中文** under Display → Interface → Language, or set `language=zh` in
the `[Settings]` section of `NVForever.ini`. This selects NVForever's UTF-8 interface
text. Item names, dialogue, quests and other game text come from the installed plugins,
decoded with the game's text code page. An English install still supplies English
item names. Reading Chinese game content requires localized plugins and a build that
supports their text encoding.

The game's 256-glyph bitmap fonts cover a single-byte code page. Windows-1252 covers
German, French, Italian and Spanish in the shipped files. Windows-1251 covers Russian.
Every file here keeps to its code page's letters (no `œ`).
The bundled Sharp menu face covers Latin and Cyrillic. Displaying the Simplified
Chinese translation requires a build with CJK font fallback and a font with the
needed glyphs. On those builds, NVForever can use a system font or a TrueType or
OpenType font or collection specified by `[Interface] CjkFontPath` in
`NVForever.ini`. The translation file itself only supplies text.
