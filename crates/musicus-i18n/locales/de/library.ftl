# Bibliothek, Stück- und Stimmeneditor. German draft, needs maintainer review.

## Library list
library-title = Bibliothek
library-search = Titel, Komponist, Schlagwort, Nummer suchen …
library-sort = Sortieren nach
library-sort-title = Nach Titel sortieren
library-sort-number = Nach Archivnummer sortieren
library-import-folder = Ordner importieren
library-import-folder-hint = Einen ganzen Ordner mit PDF-Stimmen importieren
library-add-piece = Stück hinzufügen
library-nothing-found = Nichts gefunden.
library-empty-manage = Die Bibliothek ist leer. Füge das erste Stück mit „+ Stück hinzufügen“ hinzu.
library-empty = Für dich wurden noch keine Noten freigegeben.
library-archive-number = Archivnummer
library-archive-number-badge = Nr. { $number }
library-no-parts = noch keine Stimmen
library-parts-without-pdf = { $count ->
    [one] { $count } Stimme ohne PDF
   *[other] { $count } Stimmen ohne PDF
}
library-edit-piece = Dieses Stück bearbeiten
library-edit-part = Diese Stimme bearbeiten
library-no-file = Noch keine Datei hochgeladen

## Guided flow "Add piece" and piece editor
library-crumb = ‹ Bibliothek
library-steps = Fortschritt
library-step-details = Angaben
library-step-parts = Stimmen und PDFs
library-details-hint = Nur der Titel ist Pflicht. Kategorie und Schlagwörter helfen den Musikern, das Stück zu finden.
library-next-parts = Weiter: Stimmen ›
library-parts-of = Stimmen von „{ $title }“
library-parts-hint = Lege pro Notenblatt eine Stimme an: Instrumente, Stimmung und Stimmnummern ergeben ihren Namen, z. B. „Trompete in B 1“; ein eigener Name ist optional. Gruppen erhalten Zugriff über die Instrumente.
library-parts-none = Noch keine Stimmen.
library-add-first-part = Erste Stimme hinzufügen
library-add-another-part = Weitere Stimme hinzufügen
library-back-details = ‹ Angaben
library-waiting-uploads = Warte auf Uploads …
library-finish = Fertigstellen
library-edit-piece-title = Stück bearbeiten
library-done = Fertig
library-save-details = Angaben speichern
library-parts-count = Stimmen ({ $count })
library-add-part-title = Stimme hinzufügen
library-delete-piece = Stück löschen
library-delete-piece-confirm = Dieses Stück mit allen Stimmen löschen? Die Anmerkungen aller Mitglieder dazu werden ebenfalls gelöscht.
library-delete-piece-text = Löscht das Stück mit allen Stimmen und Dateien aus der Bibliothek. { $warning } Programme behalten ihre anderen Stücke.
library-delete-piece-warning = Die Anmerkungen aller Mitglieder dazu werden ebenfalls gelöscht.
library-edit-part-title = Stimme bearbeiten
library-edit-part-more = Andere Stimmen, die Angaben zum Stück und Löschen:{" "}

## Piece fields
library-field-title = Titel
library-title-placeholder = z. B. Florentiner Marsch
library-field-subtitle = Untertitel
library-field-composer = Komponist
library-field-arranger = Arrangeur
library-field-genre = Kategorie
library-genre-placeholder = Marsch, Polka, Konzertwerk …
library-field-tags = Schlagwörter
library-tags-placeholder = z. B. Weihnachten, Zugabe
library-tags-hint = Nach jedem Schlagwort Komma oder Enter drücken.
library-field-notes = Notizen

## Part fields
library-no-instruments = Noch keine Instrumente.{" "}
library-no-instruments-link = Lege sie auf der Seite Instrumente an.
library-part-no-instruments = Noch keine Instrumente: Niemand erhält diese Stimme über eine Instrument-Freigabe.
library-field-key = Stimmung
library-key-none = — (klingend)
library-key-other = Andere: { $key }
library-field-voices = Stimmnummern
library-voices-placeholder = z. B. 1, 2 oder Solo
library-field-custom-name = Eigener Name (optional)
library-custom-name-placeholder = z. B. Solo-Flügelhorn
library-shown-as = Angezeigt als:{" "}
library-instruments-saved = Instrumente (werden sofort gespeichert)

## Files of a part
library-file-unreadable = Die Datei konnte nicht gelesen werden: { $error }
library-file-version = Version { $version } · { $size }
library-file-uploaded = { $file } · hochgeladen { $time }
library-open = Öffnen
library-edit-systems = Notenzeilen bearbeiten
library-no-pdf = Noch kein PDF
library-uploading = Wird hochgeladen …
library-replace-pdf = PDF ersetzen …
library-upload-pdf = PDF hochladen …
library-uploading-file = { $name } wird hochgeladen …
library-uploaded-version = Als Version { $version } hochgeladen
library-all-versions = Alle Versionen ({ $count })
library-version = Version { $version }
library-version-current = aktuell
library-versions-hint = Alte Versionen bleiben gespeichert, so bleiben Anmerkungen darauf erhalten und können auf die aktuelle Version übertragen werden.
library-save-part = Stimme speichern
library-delete-part = Stimme löschen
library-delete-part-confirm = Diese Stimme löschen? Die Anmerkungen aller Mitglieder dazu werden ebenfalls gelöscht.
library-choose-pdf = PDF auswählen …
library-choose-another-pdf = Anderes PDF auswählen …
library-no-file-chosen = Keine Datei ausgewählt. Du kannst sie auch später hochladen.
library-add-part = + Stimme hinzufügen
library-upload-failed = Hochladen fehlgeschlagen: { $error }
