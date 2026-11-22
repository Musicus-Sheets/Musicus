# Glossary of UI terms (English ↔ German)

For everyone who writes or translates user-interface messages (`crates/musicus-i18n/locales/*/*.ftl`,
d-i18n-fluent). Use exactly these words; a new recurring term is added here first (state `proposed`) and becomes
`agreed` only when the user (or a musician reviewer) confirms it. User data (titles, instrument names, voices, …)
is never translated (a-data-as-entered).

**German ships as an agent draft (a-i18n-german): it needs the maintainer's review.** Every German message was
written by a coding agent from this table; nothing but the `agreed` rows has been confirmed by a musician. Review aid:
`python3 scripts/i18n-review.py de > review-de.md` (key | note | English | German; `--only-flagged` lists messages
identical to English). Open choices: § Open choices below.

State: **agreed** = confirmed by the user (2026-11-20); **proposed** = suggestion, used in the German draft, confirm
during the review; **draft decision** = chosen by the agent for the draft, marked for the maintainer's review.

## Form of address and style (draft decision, review)

- **"du"** (informal, lower case), e.g. "Wähle ein Passwort", "Deine anderen Geräte wurden abgemeldet". Reason: an
  amateur ensemble's own tool; members of a Musikverein say "du" to each other, and current German app UIs mostly do.
  Change to "Sie" = one pass over `locales/de` (imperatives and "dein/deine"); keep it consistent.
- Buttons and menu entries: infinitive without "du" ("Speichern", "Stück hinzufügen", "Als Standard festlegen").
- Ellipsis with a space before it ("Wird geladen …", "PDF hochladen …"); quotes „…“; dash "–" with spaces.
- Generic masculine for roles and people (Musiker, Notenwart, Komponist, Administrator, Dirigent): short labels;
  see Open choices.
- Plurals with selectors also where English has none in a fixed sentence (`import-batch-line`, `import-undone`, …).

## Terms

| English (UI) | Meaning in Musicus | German | State |
| --- | --- | --- | --- |
| part | one sheet of a piece for one or more instruments (`parts`) | Stimme | agreed |
| voice (field "Voices") | the voice/chair number(s) of a part: "1", "2", "3a", "Solo" | Stimmnummer (Pl. Stimmnummern) | agreed |
| librarian | role that manages the library | Notenwart | agreed |
| annotation | a private marking drawn on a sheet | Anmerkung | agreed |
| programme | a concert: date, title, ordered pieces | Programm | agreed |
| key (of a part) | transposition of the instrument, "in B♭" (not the key signature) | Stimmung | proposed (never "Tonart") |
| concert pitch (no key) | non-transposing | klingend | proposed |
| piece | a work or arrangement with its parts (`scores`) | Stück | proposed |
| library | all pieces of the ensemble | Bibliothek | proposed |
| full score | instrument "Partitur" of the import dictionary | Partitur | proposed |
| conductor's part | instrument "Direktion" | Direktion | proposed |
| annotation set | a named collection of a user's annotations of one part | Anmerkungsset | proposed |
| default set | the set opened when no programme set applies | Standardset | proposed |
| instrument | entry of the instrument list | Instrument | proposed |
| instrument family | grouping of instruments ("Woodwinds") | Register | proposed |
| group | set of members that receive parts | Gruppe | proposed |
| grant | a group's permission to read parts (by instrument or piece) | Freigabe | proposed |
| member | role that reads | Mitglied | proposed |
| admin | role that manages users | Administrator | proposed |
| archive number | number of the piece in the paper archive | Archivnummer | proposed |
| genre (category) | free classification of a piece | Kategorie | proposed |
| tag | free keyword of a piece | Schlagwort | proposed |
| composer / arranger | | Komponist / Arrangeur | proposed |
| custom name | a part name typed instead of the derived one | eigener Name | proposed |
| system | one line of music (a staff or a group of braced staves) | Notenzeile | proposed |
| staff size | size of the staves in the system view | Notengröße | proposed |
| page view / system view | the two reader modes | Seitenansicht / Zeilenansicht | proposed |
| reader | the full-screen view of a part | Leseansicht | proposed |
| import (batch) | a mass import of a folder | Import | proposed |
| invite (link) | link that lets a new user set a password | Einladung(slink) | proposed |
| About Musicus | page with version, licence and source code | Über Musicus | proposed |
| version | the release that runs ("0.1.0") | Version | proposed |
| licence / source code | AGPL-3.0-or-later / the repository | Lizenz / Quellcode | proposed |
| device (session) | a logged-in browser | Gerät | proposed |
| pen / highlighter / eraser / stamp / text | reader tools | Stift / Textmarker / Radierer / Stempel / Text | proposed |
| staff | the five lines of one instrument (import notes: "1 staff per system") | Notensystem | proposed |
| band / cut (system editor) | a detected region of a page / the line between two regions | Band / Schnitt | proposed |
| header / footer (system editor) | title area / bottom area of a page | Kopf (Titel) / Fußzeile | proposed |
| pattern / preset (import) | file-name pattern / predefined structure | Muster / Vorlage | proposed |
| naming language (import) | language of the archive's file names | Sprache der Namen | proposed |
| sound check (example text) | | Anspielprobe | proposed |
| default (set tag) | | Standard | proposed |
| unnamed part / "Part 2" | derived names without instrument | Unbenannte Stimme / Stimme 2 | proposed |
| default set name "My annotations" | shown for the set stored without name | Meine Anmerkungen | proposed |
| upload | | hochladen / Hochladen | proposed |

## Key names

Keys are stored as ASCII codes (`naming::KEYS`) and shown in the UI language (a-client-part-names). The German
names follow the agreed rule "German note names: B / Es / H".

| Code | English | German | State |
| --- | --- | --- | --- |
| `Bb` | B♭ | B | agreed |
| `Eb` | E♭ | Es | agreed |
| `H` | B♮ (option "B♮ (H)") | H (option "H") | agreed |
| `Ab` | A♭ | As | follows the rule |
| `Db` | D♭ | Des | follows the rule |
| `Gb` | G♭ | Ges | follows the rule |
| `F`, `C`, `A`, `D`, `G`, `E` | F, C, A, D, G, E | F, C, A, D, G, E | follows the rule |

Derived part names: English "Clarinet in B♭ 1 & 2", German "Klarinette in B 1 & 2" (the instrument name is the
ensemble's data, only " in ", the key name and the separators follow the language).

## Open choices for the maintainer's review

1. Form of address: "du" (draft) or "Sie".
2. Generic masculine (Musiker, Notenwart, Komponist, Arrangeur, Administrator, Dirigent) or inclusive forms
   ("Musikerinnen und Musiker", "Notenwart:in"); inclusive forms make buttons and table headers longer.
3. "Notensystem" for staff (import notes, "2 Notensysteme pro Zeile") next to "Notenzeile" for system: both contain
   "System"/"Zeile" ambiguities; alternatives "Fünfliniensystem", "Akkolade" (system).
4. Reader button "☰ Zeilen" (system view) and "Zeilenansicht"; alternative "☰ Systeme".
5. "Stimmung" for key and "klingend" (no key) — the glossary's proposals, now visible in the part editor.
6. "Anmerkungsset" / "Set" for annotation set (alternatives: "Anmerkungssatz", "Ebene").
7. "Register" for instrument family (alternative "Instrumentengruppe"); "Kategorie" for genre (alternative "Gattung");
   "Schlagwort" for tag (alternative "Tag").
8. "Administrator" as role name (alternative "Admin", shorter).
9. "Freigabe" for grant ("Instrument-Freigabe").
10. Line widths "Fein / Mittel / Breit" (alternative "Dick" for bold); colour "Pink".
11. ~~Button "Link zum Zurücksetzen"~~ — decided: "Zurücksetzen" (users page, reset link; maintainer, 2026-11-21).
12. Key option "H" alone in German (English shows "B♮ (H)").
13. Examples in placeholders: "Weihnachtskonzert 2026", "Treffpunkt 18:30 Uhr zur Anspielprobe", "Solo-Flügelhorn",
    "Trompete", "Blech", "Marsch, Polka, Konzertwerk …".
