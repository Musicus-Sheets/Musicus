# Glossary of UI terms (English ↔ German)

For everyone who writes or translates user-interface messages (`crates/musicus-i18n/locales/*/*.ftl`,
d-i18n-fluent). Use exactly these words; a new recurring term is added here first (state `proposed`) and becomes
`agreed` only when the user (or a musician reviewer) confirms it. User data (titles, instrument names, voices, …)
is never translated (a-data-as-entered). The German catalogue itself comes later (its own branch); this table is
its terminology.

State: **agreed** = confirmed by the user (2026-11-20); **proposed** = suggestion, confirm before the German release.

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
| device (session) | a logged-in browser | Gerät | proposed |
| pen / highlighter / eraser / stamp / text | reader tools | Stift / Textmarker / Radierer / Stempel / Text | proposed |

## Key names

Keys are stored as ASCII codes (`naming::KEYS`) and shown in the UI language (a-client-part-names). The German
names follow the agreed rule "German note names: B / Es / H".

| Code | English | German | State |
| --- | --- | --- | --- |
| `Bb` | B♭ | B | agreed |
| `Eb` | E♭ | Es | agreed |
| `H` | B♮ (option "B♮ (H)") | H | agreed |
| `Ab` | A♭ | As | follows the rule |
| `Db` | D♭ | Des | follows the rule |
| `Gb` | G♭ | Ges | follows the rule |
| `F`, `C`, `A`, `D`, `G`, `E` | F, C, A, D, G, E | F, C, A, D, G, E | follows the rule |

Derived part names: English "Clarinet in B♭ 1 & 2", German "Klarinette in B 1 & 2" (the instrument name is the
ensemble's data, only " in ", the key name and the separators follow the language).
