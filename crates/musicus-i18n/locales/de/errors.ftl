# Fehlercodes des Servers, Importhinweise, Muster- und Anmerkungsfehler.
# German draft, needs maintainer review. Arguments that are user data are shown as entered.

## Requests
error-network = Netzwerkfehler: { $detail }
error-http-failed = Anfrage fehlgeschlagen (HTTP { $status })
error-unknown = Unbekannter Fehler

## Error codes
error-not-found = Nicht gefunden
error-unauthorized = Bitte melde dich an
error-internal = Auf dem Server ist etwas schiefgegangen. Bitte versuch es später noch einmal
error-payload-too-large = Die Datei ist zu groß (höchstens { $size })
error-too-many-attempts = Zu viele Versuche. Versuch es in { $minutes ->
    [one] { $minutes } Minute
   *[other] { $minutes } Minuten
} noch einmal
error-csrf-missing = Die Anfrage wurde abgelehnt (Sicherheitskopfzeile fehlt). Lade die Seite neu
error-librarian-only = Das dürfen nur Notenwarte
error-admin-only = Das dürfen nur Administratoren
error-already-exists = Ein Eintrag mit diesem Namen existiert bereits
error-in-use-or-unknown = Der Eintrag wird noch verwendet oder verweist auf etwas, das es nicht gibt
error-field-empty = { $field } darf nicht leer sein
error-field-too-long = { $field } ist länger als { $max } Zeichen
error-wrong-password = Das aktuelle Passwort ist falsch
error-password-too-short = Das Passwort braucht mindestens { $min } Zeichen
error-password-too-long = Das Passwort darf höchstens { $max } Zeichen haben
error-username-invalid = Benutzernamen dürfen Buchstaben, Ziffern, '.', '_' und '-' enthalten
error-cannot-lock-out-self = Du kannst dich nicht selbst deaktivieren oder dir die Administratorrolle entziehen
error-user-disabled = Aktiviere den Benutzer, bevor du ihn einlädst
error-too-many-tags = Höchstens { $max } Schlagwörter
error-too-many-instruments = Höchstens { $max } Instrumente pro Stimme
error-unknown-key = Unbekannte Stimmung „{ $key }“: wähle eine aus der Liste
error-too-many-voices = Höchstens { $max } Stimmnummern pro Stimme
error-voice-too-long = Die Stimmnummer „{ $voice }“ ist länger als { $max } Zeichen
error-upload-interrupted = Das Hochladen wurde unterbrochen. Bitte versuch es noch einmal
error-not-pdf = Nur PDF-Dateien werden unterstützt
error-systems-not-detected = Die Notenzeilen dieser Datei wurden noch nicht erkannt
error-correction-too-many-bands = Höchstens { $max } Bänder pro Seite
error-correction-invalid = Die korrigierten Bänder sind ungültig. Lade die Seite neu und versuch es noch einmal
error-unknown-programme = Unbekanntes Programm
error-copy-from-other-part = Das zu kopierende Set gehört zu einer anderen Stimme
error-part-has-no-file = Diese Stimme hat noch keine Datei
error-page-count-changed = Die neue Dateiversion hat eine andere Seitenzahl
error-if-match = Die Anmerkungen konnten nicht gespeichert werden (Revision fehlt)
error-date-format = Das Datum muss wie 2026-12-24 aussehen
error-too-many-pieces = Höchstens { $max } Stücke pro Programm
error-invalid-path = Ungültiger Pfad „{ $path }“
error-not-pdf-name = Kein PDF-Dateiname: „{ $path }“
error-too-many-files = Höchstens { $max } Dateien pro Anfrage
error-invalid-file-hash = Ungültige Prüfsumme oder Größe für „{ $path }“
error-file-changed = Die Datei hat sich nach der Prüfung geändert; wähle den Ordner erneut aus
error-item-count = Wähle 1 bis { $max } Einträge
error-unknown-items = Unbekannte Einträge oder Einträge aus mehreren Importen
error-import-committed = Bereits importiert; mach den Import rückgängig, um ihn zu ändern
error-unknown-score = Unbekanntes Stück
error-unknown-instruments = Unbekannte Instrumente
error-unknown-instrument = Unbekanntes Instrument
error-file-not-uploaded = { $path } ist nicht hochgeladen
error-undo-annotated = Rückgängig machen nicht möglich: Musiker haben Anmerkungen auf importierten Stimmen von { $pieces } gemacht
error-undo-changed = Rückgängig machen nicht möglich: Diese importierten Stücke haben inzwischen andere Stimmen bekommen oder sind in einem Programm: { $pieces }
error-not-in-batch = Kein Stück dieses Imports
error-duplicate-in-library = Schon in der Bibliothek: { $title }
error-duplicate-in-batch = Dieselbe Datei wie { $path }
error-import-not-ready = Nicht bereit

## Field names in error messages (as the forms label them)
error-field-name = Name
error-field-family = Register
error-field-username = Benutzername
error-field-display-name = Anzeigename
error-field-title = Titel
error-field-subtitle = Untertitel
error-field-composer = Komponist
error-field-arranger = Arrangeur
error-field-genre = Kategorie
error-field-notes = Notizen
error-field-archive-number = Archivnummer
error-field-tag = Ein Schlagwort
error-field-label = Eigener Name
error-field-date = Datum
error-field-pattern = Muster
error-field-instrument-word = Ein Instrumentenwort
error-field-alias = Der Alias

## Import notes: lower case where German allows it (they often follow other text).
note-ambiguous-key = „{ $key }“ ist nur im Deutschen ein Stimmungsname (B = B♭, Es = E♭, H = B♮): wähle die Stimmung oder stelle die Sprache des Imports auf Deutsch
note-unknown-key = „{ $key }“ ist keine bekannte Stimmung
note-invalid-key = unbekannte Stimmung „{ $key }“
note-two-keys = mehrere Stimmungen im Namen: { $keys }
note-several-voices = Zahlen ohne Trennzeichen im Namen ({ $voices }): prüfe die Stimmnummern (oder ist es eine Kopie- oder Seitennummer?)
note-abbreviation = „{ $word }“ ist eine Abkürzung: prüfe das Instrument
note-guessed = „{ $word }“ wurde aus dem Wortanfang erraten: prüfe das Instrument
note-ambiguous-instrument = „{ $word }“ kann mehrere Instrumente bedeuten: prüfe es
note-unknown-instrument = unbekanntes Instrument „{ $word }“: wähle ein Instrument oder lege es an
note-no-instrument = kein Instrument: niemand kann diese Stimme über eine Instrument-Freigabe erhalten
note-unrecognised = nicht verstanden: „{ $text }“
note-contradictory = die Dateien dieses Stücks widersprechen sich: { $values }
note-title-missing = die Dateinamen enthalten keinen Titel: gib ihn ein
note-similar-title = es gibt ein Stück mit ähnlichem Titel: { $piece }
note-confirm-target = das könnte das vorhandene Stück { $piece } sein: füge die Stimmen dort hinzu oder importiere es als neues Stück
note-target-gone = das gewählte vorhandene Stück wurde gelöscht
note-no-parts = keine Stimmen zum Importieren
note-not-uploaded = Datei noch nicht hochgeladen: wähle den Ordner erneut aus, um fortzufahren
note-upload-failed-because = Hochladen fehlgeschlagen: { $error }
note-upload-failed = Hochladen fehlgeschlagen
note-score-with-one-staff = als Partitur benannt, aber ihre Notenzeilen haben nur ein Notensystem
note-many-staves = eine einzelne Stimme, aber ihre Notenzeilen haben { $count } Notensysteme: eine Partitur?
note-no-readers = noch keine Gruppe kann diese Stimme lesen

## File name patterns of the import (placeholders are never translated)
error-pattern-unknown-placeholder = unbekannter Platzhalter { $placeholder }; verwende { $known }
error-pattern-unbalanced = eckige Klammern [ ] oder geschweifte Klammern {"{"} {"}"} passen nicht zusammen
error-pattern-missing = das Muster braucht { $placeholder }
error-pattern-missing-title-or-number = das Muster braucht { $title } oder { $number }
error-pattern-twice = { $placeholder } darf nur einmal vorkommen
error-pattern-too-many-optionals = höchstens { $max } optionale Gruppen [ ]

## Annotation pages
error-ink-damaged = Die Anmerkungen dieser Seite sind beschädigt
error-ink-newer-version = Die Anmerkungen wurden von einer neueren Version von Musicus gespeichert
error-ink-too-large = Die Anmerkungen dieser Seite sind zu groß (höchstens { $size })
error-ink-too-many-objects = Zu viele Anmerkungen auf dieser Seite (höchstens { $max })
error-ink-stroke-too-long = Ein Strich ist zu lang (höchstens { $max } Punkte)
error-ink-text-too-long = Ein Text ist zu lang (höchstens { $max } Zeichen)
