//! Prints the schema snapshot (`crates/musicus-server/schema.sql`, a-db-check):
//! creates an empty database in a temporary folder with `Db::open`, i.e. with
//! the update runner and the updates built into this program, exactly as
//! Musicus creates a new database, and prints `updates::schema::snapshot`.
//! Run through `scripts/db-schema.sh` (writes or checks the file):
//!
//!     cargo run -q -p musicus-server --example db_schema > crates/musicus-server/schema.sql

use musicus_server::{db::Db, updates::schema};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let dir = tempfile::tempdir().expect("temporary folder");
    let (db, _) = Db::open(&dir.path().join("musicus.sqlite"), 1).await.expect("database updates");
    print!("{}", schema::snapshot(&db.read).await.expect("read the schema"));
}
