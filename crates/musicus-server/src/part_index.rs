//! Search index of parts (`part_fts`, preview `migrations/preview/0002-part-names.sql`). The server composes
//! no part names (a-client-part-names): it indexes the components (custom
//! name, instrument names as entered, voices) plus the key code and every
//! name of the key in the shipped languages (`musicus_i18n::KEY_SPELLINGS`),
//! so a part is found by whatever words a reader of any language types
//! (R-I18N-007). Instrument names live in another table, so SQL triggers
//! cannot keep the text current; the server re-indexes parts whenever a part
//! or an instrument changes, and rebuilds the whole index on start when it
//! does not match the parts table (after an update that empties it) or when
//! another program used the database last.
//! Deleting a part removes its row by trigger. (a-part-names, R-LIB-011)

use crate::db::json_ids;
use musicus_api::Id;
use sqlx::SqliteConnection;

#[derive(sqlx::FromRow)]
struct Row {
    id: Id,
    label: Option<String>,
    voices: String,
    transposition: Option<String>,
    instruments: String,
}

/// Indexed text of a part: custom name, instrument names, voices (each its
/// own token, so "clarinet 2" finds "Clarinet 1 & 2", a-part-voices), the key
/// code ("Bb") and the key's names in all shipped languages ("B♭"; FTS5
/// splits at "♭", giving the token "b").
pub fn index_text(label: Option<&str>, instruments: &[&str], key: Option<&str>, voices: &[String]) -> String {
    index_text_with(label, instruments, key, voices, musicus_i18n::key_spellings)
}

fn index_text_with(
    label: Option<&str>,
    instruments: &[&str],
    key: Option<&str>,
    voices: &[String],
    spellings: impl Fn(&str) -> &'static [&'static str],
) -> String {
    let mut words: Vec<&str> = Vec::new();
    words.extend(label.map(str::trim).filter(|l| !l.is_empty()));
    words.extend(instruments.iter().map(|i| i.trim()));
    words.extend(voices.iter().map(|v| v.trim()));
    if let Some(k) = key.map(str::trim).filter(|k| !k.is_empty()) {
        words.push(k);
        words.extend(spellings(k).iter().copied().filter(|s| *s != k));
    }
    words.retain(|w| !w.is_empty());
    words.join(" ")
}

/// Re-indexes the given parts (all parts with `None`). Runs inside the
/// caller's write transaction.
pub async fn reindex(conn: &mut SqliteConnection, part_ids: Option<&[Id]>) -> sqlx::Result<()> {
    let ids = part_ids.map(json_ids);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT p.id, p.label, p.transposition,
                (SELECT json_group_array(json_array(position, voice)) FROM part_voices WHERE part_id = p.id) AS voices,
                (SELECT json_group_array(i.name) FROM part_instruments pi JOIN instruments i ON i.id = pi.instrument_id
                 WHERE pi.part_id = p.id) AS instruments
         FROM parts p WHERE ?1 IS NULL OR p.id IN (SELECT value FROM json_each(?1))",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM part_fts WHERE ?1 IS NULL OR rowid IN (SELECT value FROM json_each(?1))")
        .bind(&ids)
        .execute(&mut *conn)
        .await?;
    for r in rows {
        let names: Vec<String> = serde_json::from_str(&r.instruments).unwrap_or_default();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let voices = crate::routes::library::voices_from_json(&r.voices);
        let text = index_text(r.label.as_deref(), &names, r.transposition.as_deref(), &voices);
        sqlx::query("INSERT INTO part_fts (rowid, name) VALUES (?1, ?2)")
            .bind(r.id)
            .bind(text)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Rebuilds the index when `force` (another program used the database last,
/// d-update-model) or when its row count differs from the parts table.
pub async fn rebuild_if_needed(db: &crate::db::Db, force: bool) -> sqlx::Result<bool> {
    let stale: bool = force
        || sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM parts) != (SELECT COUNT(*) FROM part_fts)")
            .fetch_one(&db.write)
            .await?;
    if stale {
        let mut tx = db.write.begin().await?;
        reindex(&mut tx, None).await?;
        tx.commit().await?;
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_text_has_components_and_key_spellings() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(index_text(None, &["Clarinet"], Some("Bb"), &v(&["2"])), "Clarinet 2 Bb B♭ B");
        assert_eq!(index_text(Some("Solo Cornet"), &["Trumpet"], Some("Bb"), &[]), "Solo Cornet Trumpet Bb B♭ B");
        assert_eq!(index_text(None, &["Horn"], Some("Eb"), &[]), "Horn Eb E♭ Es", "German name of E♭");
        assert_eq!(index_text(None, &["Posaune"], Some("H"), &[]), "Posaune H B♮", "the code is the German name");
        assert_eq!(index_text(None, &["Flute"], None, &[]), "Flute");
        assert_eq!(index_text(None, &["Klarinette"], None, &v(&["1", "2"])), "Klarinette 1 2");
        assert_eq!(index_text(None, &["Horn"], Some("Es/F"), &[]), "Horn Es/F", "legacy keys as stored");
        // Another table of spellings (a further language) adds its names.
        let nl = |k: &str| -> &'static [&'static str] {
            match k {
                "Bb" => &["Bes"],
                _ => &[],
            }
        };
        assert_eq!(index_text_with(None, &["Klarinet"], Some("Bb"), &[], nl), "Klarinet Bb Bes");
    }

    /// The spellings the index of existing installations was last rebuilt
    /// with (development migration 0010, now preview `0010-german-key-spellings`).
    /// When a catalogue changes a key name or a language is added, the stored
    /// index lacks the new spellings: add a preview update whose additive part
    /// is `DELETE FROM part_fts;` and update this table.
    const INDEXED_SPELLINGS: &[(&str, &[&str])] = &[
        ("A", &["A"]),
        ("Ab", &["A♭", "As"]),
        ("C", &["C"]),
        ("D", &["D"]),
        ("Db", &["D♭", "Des"]),
        ("E", &["E"]),
        ("Eb", &["E♭", "Es"]),
        ("F", &["F"]),
        ("G", &["G"]),
        ("Gb", &["G♭", "Ges"]),
        ("H", &["B♮", "H"]),
        ("Bb", &["B♭", "B"]),
    ];

    #[test]
    fn indexed_key_spellings_are_current() {
        let mut want: Vec<_> = INDEXED_SPELLINGS.to_vec();
        let mut got: Vec<_> = musicus_i18n::KEY_SPELLINGS.to_vec();
        want.sort();
        got.sort();
        assert_eq!(
            got, want,
            "key spellings changed: add a preview update that empties part_fts, then update the table"
        );
    }
}
