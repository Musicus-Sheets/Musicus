# Massenimport: Importliste, neuer Import mit Vorschau, Prüfung. German draft, needs maintainer review.

## Import page
import-title = Import
import-intro = Importiere einen ganzen Ordner mit PDF-Stimmen: Musicus liest Nummer, Titel, Komponist und die Stimmen aus den Ordner- und Dateinamen. Nichts gelangt in die Bibliothek, bevor du es geprüft und importiert hast.
import-earlier = Frühere Importe
import-none = Noch keine Importe.
import-forget-confirm = Diesen Import vergessen? Bereits importierte Stücke bleiben in der Bibliothek.
import-batch-line = { $time }{ $by } · { $pieces ->
    [one] { $pieces } Stück
   *[other] { $pieces } Stücke
}, { $committed } importiert · { $files ->
    [one] { $files } Datei
   *[other] { $files } Dateien
}
import-batch-by = {" "}von { $name }
import-batch-missing = { $count ->
    [one] { $count } Datei noch nicht hochgeladen
   *[other] { $count } Dateien noch nicht hochgeladen
}
import-review = Prüfen
import-forget = Vergessen
import-select-folder = Ordner auswählen …
import-drop-folder = oder den Ordner hierher ziehen
import-read-failed = { $path } konnte nicht gelesen werden: { $error }

## Progress of hashing and uploading
import-progress-hashing = Dateien werden geprüft … { $done } von { $total }
import-progress-registering = Abgleich mit der Bibliothek …
import-progress-uploading = { $done } von { $total } werden hochgeladen{ $failed ->
    [0] {""}
   *[other] {" "}({ $failed } fehlgeschlagen)
}: { $name }
import-progress-finished = Hochladen abgeschlossen: { $uploaded } hochgeladen{ $skipped ->
    [0] {""}
   *[other] , { $skipped } übersprungen (schon in der Bibliothek oder doppelt im Ordner)
}{ $failed ->
    [0] {""}
   *[other] , { $failed } fehlgeschlagen – wähle den Ordner erneut aus, um es noch einmal zu versuchen
}

## New import
import-new = Neuer Import
import-pdf-files = { $count ->
    [one] { $count } PDF-Datei
   *[other] { $count } PDF-Dateien
}
import-in-folder = {" "}in „{ $folder }“
import-ignored = { $count ->
    [one] {" "}(1 andere Datei wird ignoriert)
   *[other] {" "}({ $count } andere Dateien werden ignoriert)
}
import-structure = Struktur
import-structure-option = { $preset } – { $matched } von { $total } Dateien
import-own-pattern = Eigenes Muster
import-preset-folder = Ein Ordner pro Stück
import-preset-flat-number = Nummer und Stimme im Dateinamen
import-preset-flat-title = Titel und Stimme im Dateinamen
import-naming-language = Sprache der Namen
import-language-de = Deutsch: B = B♭, Es = E♭, H = B♮
import-language-de-suggested = Deutsch (vorgeschlagen): B = B♭, Es = E♭, H = B♮
import-language-en = Englisch: B wird nicht als Stimmung gelesen
import-language-en-suggested = Englisch (vorgeschlagen): B wird nicht als Stimmung gelesen
import-pattern = Muster
import-pattern-hint = { $placeholders }, { $ignore } = ignorieren, [ … ] = optional, / = Ordner
import-summary-pieces = { $count ->
    [one] { $count } Stück:{" "}
   *[other] { $count } Stücke:{" "}
}
import-count-ready = { $count } bereit
import-count-check = { $count } zu prüfen
import-count-complete = { $count } zu ergänzen
import-count-unmatched = { $count ->
    [one] { $count } Datei passt nicht zum Muster
   *[other] { $count } Dateien passen nicht zum Muster
}
import-unmatched-files = Dateien, die nicht zum Muster passen
import-size-to-check = { $size } zu prüfen. Du kannst alles prüfen, bevor etwas importiert wird.
import-upload-and-review = Hochladen und prüfen
import-col-number = Nr.
import-col-parts = Stimmen
import-title-missing = — Titel fehlt —
import-more-pieces = … und { $count ->
    [one] { $count } weiteres Stück.
   *[other] { $count } weitere Stücke.
}

## Review
import-crumb = ‹ Import
import-source-pattern = aus dem Dateinamen
import-source-dictionary = aus dem Instrumentenwörterbuch
import-source-alias = aus einem gespeicherten Alias
import-source-library = aus der Bibliothek
import-source-librarian = von dir eingegeben
import-source-inventory = aus der Inventarliste
import-source-ocr = auf der ersten Seite erkannt
import-also = {" "}(auch: { $values })
import-committed = { $count ->
    [one] { $count } Stück importiert.
   *[other] { $count } Stücke importiert.
}
import-not-committed = {" "}Nicht importiert: { $error }.
import-undo-confirm = Diesen Import rückgängig machen? Die erzeugten Stücke und die hinzugefügten Stimmen werden aus der Bibliothek entfernt (die Dateien bleiben auf dem Server). Das wird abgelehnt, wenn Musiker schon Anmerkungen darauf gemacht haben.
import-undone = Rückgängig gemacht: { $pieces ->
    [one] { $pieces } Stück
   *[other] { $pieces } Stücke
} und { $parts ->
    [one] { $parts } Stimme
   *[other] { $parts } Stimmen
} entfernt.
import-count-imported = { $count } importiert
import-files-summary = { $total } Dateien: { $uploaded } hochgeladen, { $duplicate } übersprungen (schon in der Bibliothek oder doppelt im Ordner), { $unmatched } passen nicht zum Muster
import-resume = { $count ->
    [one] { $count } Datei ist noch nicht hochgeladen.
   *[other] { $count } Dateien sind noch nicht hochgeladen.
} Wähle denselben Ordner noch einmal aus, um fortzufahren; bereits hochgeladene Dateien werden nicht erneut gesendet.
import-filter = Anzeigen
import-filter-all = Alle Stücke
import-filter-attention = Zu prüfen
import-filter-ready = Bereit zum Import
import-filter-imported = Importiert
import-commit-green = Alle grünen importieren ({ $count })
import-undo = Import rückgängig machen
import-previous = ‹ Zurück
import-next = Weiter ›
import-page = Seite { $page } von { $count }
import-duplicates = { $count ->
    [one] { $count } Datei übersprungen (Duplikat)
   *[other] { $count } Dateien übersprungen (Duplikate)
}

## Batch settings
import-language-name-de = Deutsch
import-language-name-en = Englisch
import-settings-pattern = Muster{" "}
import-settings-language = {" "}· Sprache der Namen { $language }{" "}
import-settings-change = Ändern
import-presets = Vorlagen:{" "}
import-read-again = Namen neu einlesen
import-read-again-confirm = Die Dateinamen neu einlesen? Änderungen an Stücken, deren Gruppierung sich ändert, gehen verloren; Änderungen an Stimmen bleiben erhalten.

## Instruments to confirm
import-confirm-title = Zu bestätigende Instrumente
import-confirm-intro = Musicus legt nie selbst Instrumente an: Das Instrument entscheidet, wer eine Stimme lesen kann. Lege einmal fest, was jedes Wort bedeutet; Musicus merkt es sich für diesen und jeden späteren Import.
import-word-count = {" "}{ $kind ->
    [unknown] unbekannt
   *[abbreviation] Abkürzung
}, in { $count ->
    [one] { $count } Datei
   *[other] { $count } Dateien
}
import-word-probably = {" "}– wahrscheinlich { $instrument }
import-word-instrument = Instrument für { $word }
import-word-choose = — Instrument auswählen —
import-word-use = Für „{ $word }“ verwenden
import-word-create = Instrument „{ $name }“ anlegen

## Bulk edit
import-bulk-hint = Kreuze Stücke an, um ihnen auf einmal dieselbe Kategorie, denselben Komponisten oder Arrangeur zu geben.
import-bulk-apply = Auf { $count ->
    [one] { $count } Stück
   *[other] { $count } Stücke
} anwenden
import-bulk-clear = Auswahl aufheben

## Piece and part rows
import-select-piece = Stück auswählen
import-field-number = Nummer
import-piece-done = ✓ Importiert
import-open-piece = Stück öffnen
import-commit-piece = Stück importieren
import-match = Ein vorhandenes Stück passt: { $piece }.{" "}
import-match-add = {" "}Die Stimmen dort hinzufügen
import-match-new = {" "}Als neues Stück importieren
import-col-page = Seite 1
import-col-file = Datei
import-col-name = Name
import-col-readers = Lesbar für
import-col-import = Import
import-staves = { $count ->
    [one] , 1 Notensystem pro Zeile
   *[other] , { $count } Notensysteme pro Zeile
}
import-pages-short = { $count } S.
import-open-pdf = PDF öffnen
import-instruments-of = Instrumente von { $path }
import-remove-instrument = Instrument { $name } entfernen
import-add-instrument = + Instrument hinzufügen
import-all-instruments-chosen = Alle Instrumente sind schon ausgewählt.
import-key-of = Stimmung von { $path }
import-readers-none = noch niemand
import-take = { $path } importieren
import-voices-of = Stimmnummern von { $path }
