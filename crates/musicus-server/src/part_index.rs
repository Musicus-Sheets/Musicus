//! Search index of part names (`part_fts`, migration 0002). A part's shown
//! name depends on the names of its instruments, so SQL triggers cannot
//! keep it current; the server re-indexes parts whenever a part or an
//! instrument changes, and rebuilds the whole index on start when it does
//! not match the parts table (first start after the migration).
//! Deleting a part removes its row by trigger. (a-part-names, R-LIB-011)

use crate::db::json_ids;
use musicus_api::{Id, naming};
use sqlx::SqliteConnection;

#[derive(sqlx::FromRow)]
struct Row {
    id: Id,
    label: Option<String>,
    voices: String,
    transposition: Option<String>,
    instruments: String,
}

/// Indexed text of a part: its shown name, the derived name when a custom
/// name hides it, and the key code ("Bb"), which the glyph form "B♭" does
/// not tokenise to. Every voice is its own token ("Clarinet 1 & 2" gives
/// "1" and "2"), so "clarinet 2" finds the part (a-part-voices).
pub fn index_text(label: Option<&str>, instruments: &[&str], key: Option<&str>, voices: &[String]) -> String {
    let derived = naming::derived_name(instruments, key, voices);
    let mut text = naming::part_name(label, instruments, key, voices);
    if text != derived {
        text.push(' ');
        text.push_str(&derived);
    }
    if let Some(k) = key {
        text.push(' ');
        text.push_str(k);
    }
    text
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

/// Rebuilds the index when its row count differs from the parts table.
pub async fn rebuild_if_needed(db: &crate::db::Db) -> sqlx::Result<bool> {
    let stale: bool = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM parts) != (SELECT COUNT(*) FROM part_fts)")
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
    fn index_text_has_derived_name_and_key_code() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(index_text(None, &["Clarinet"], Some("Bb"), &v(&["2"])), "Clarinet in B♭ 2 Bb");
        assert_eq!(index_text(Some("Solo Cornet"), &["Trumpet"], Some("Bb"), &[]), "Solo Cornet Trumpet in B♭ Bb");
        assert_eq!(index_text(None, &["Flute"], None, &[]), "Flute");
        assert_eq!(index_text(None, &["Klarinette"], None, &v(&["1", "2"])), "Klarinette 1 & 2");
    }
}
