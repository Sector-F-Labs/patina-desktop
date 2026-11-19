//! The file: a SQLite database (ADR-0002).
//!
//! Opening reads everything into memory. Edits go to a writer thread that
//! owns the connection, so the window never waits on the disk; dropping the
//! [`Writer`] finishes the queue first.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use rusqlite::{Connection, params};

use crate::formula::Pos;
use crate::shell::{Run, Status};
use crate::sheet::Sheet;

/// `PRAGMA application_id`: "LDGR". It tells a Ledger file from any other
/// SQLite database.
pub const APPLICATION_ID: i32 = 0x4C44_4752;
/// `PRAGMA user_version`: bumped when the schema changes.
pub const SCHEMA_VERSION: i32 = 2;

const SCHEMA: &str = "
CREATE TABLE sheets (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    position INTEGER NOT NULL
);
CREATE TABLE cells (
    sheet_id INTEGER NOT NULL REFERENCES sheets(id) ON DELETE CASCADE,
    row      INTEGER NOT NULL,
    col      INTEGER NOT NULL,
    input    TEXT NOT NULL,
    PRIMARY KEY (sheet_id, row, col)
) WITHOUT ROWID;
";

/// Added in format 2 (ADR-0006): what shell formulas last printed.
const SHELL_RESULTS: &str = "
CREATE TABLE IF NOT EXISTS shell_results (
    sheet_id INTEGER NOT NULL REFERENCES sheets(id) ON DELETE CASCADE,
    row      INTEGER NOT NULL,
    col      INTEGER NOT NULL,
    formula  TEXT NOT NULL,
    status   TEXT NOT NULL,
    stdout   TEXT NOT NULL,
    stderr   TEXT NOT NULL,
    ran_at   INTEGER NOT NULL,
    PRIMARY KEY (sheet_id, row, col)
) WITHOUT ROWID;
";

/// A file read into memory, and the way to write changes back.
pub struct Document {
    pub path: PathBuf,
    pub sheets: Vec<Sheet>,
    pub writer: Writer,
}

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// Makes a new file with one empty sheet. Refuses to overwrite one.
pub fn create(path: &Path) -> Result<Document, String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let conn = Connection::open(path).map_err(err)?;
    conn.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = {SCHEMA_VERSION};"))
        .map_err(err)?;
    conn.execute_batch(SCHEMA).map_err(err)?;
    conn.execute_batch(SHELL_RESULTS).map_err(err)?;
    conn.execute("INSERT INTO sheets (id, name, position) VALUES (1, 'Sheet 1', 0)", []).map_err(err)?;
    Ok(Document { path: path.to_path_buf(), sheets: vec![Sheet::new(1, "Sheet 1")], writer: Writer::start(conn) })
}

/// Reads a Ledger file. Any other SQLite database is refused, not changed.
pub fn open(path: &Path) -> Result<Document, String> {
    if !path.is_file() {
        return Err(format!("{} isn't a file", path.display()));
    }
    let conn = Connection::open(path).map_err(err)?;
    let id: i32 = conn.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|_| not_ledger(path))?;
    if id != APPLICATION_ID {
        return Err(not_ledger(path));
    }
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
    if version > SCHEMA_VERSION {
        return Err(format!("{} was made by a newer Ledger (format {version})", path.display()));
    }
    if version < 2 {
        // Format 1 had no shell results. The change is additive, so older
        // Ledgers still read the file's sheets and cells.
        conn.execute_batch(SHELL_RESULTS).map_err(err)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};")).map_err(err)?;
    }
    let mut sheets = Vec::new();
    {
        let mut st = conn.prepare("SELECT id, name FROM sheets ORDER BY position, id").map_err(err)?;
        let rows: Vec<(i64, String)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<Result<_, _>>().map_err(err)?;
        let mut cells = conn.prepare("SELECT row, col, input FROM cells WHERE sheet_id = ?1").map_err(err)?;
        let mut shell = conn
            .prepare("SELECT row, col, formula, status, stdout, stderr, ran_at FROM shell_results WHERE sheet_id = ?1")
            .map_err(err)?;
        for (id, name) in rows {
            let map: BTreeMap<Pos, String> = cells
                .query_map([id], |r| Ok(((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?), r.get::<_, String>(2)?)))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
            let mut runs = HashMap::new();
            for row in shell.query_map([id], |r| {
                Ok((
                    (r.get::<_, u32>(0)?, r.get::<_, u32>(1)?),
                    Run {
                        formula: r.get(2)?,
                        status: Status::decode(&r.get::<_, String>(3)?),
                        stdout: r.get(4)?,
                        stderr: r.get(5)?,
                        ran_at: r.get(6)?,
                    },
                ))
            })
            .map_err(err)?
            {
                let (pos, run) = row.map_err(err)?;
                runs.insert(pos, run);
            }
            sheets.push(Sheet::with_cells(id, name, map, runs));
        }
    }
    if sheets.is_empty() {
        conn.execute("INSERT INTO sheets (id, name, position) VALUES (1, 'Sheet 1', 0)", []).map_err(err)?;
        sheets.push(Sheet::new(1, "Sheet 1"));
    }
    Ok(Document { path: path.to_path_buf(), sheets, writer: Writer::start(conn) })
}

fn not_ledger(path: &Path) -> String {
    format!("{} isn't a Ledger file", path.display())
}

/// One change to the file.
#[derive(Debug)]
pub enum Write {
    /// Set a cell; empty input deletes it.
    Cell { sheet: i64, pos: Pos, input: String },
    AddSheet { id: i64, name: String },
    RenameSheet { id: i64, name: String },
    DeleteSheet { id: i64 },
    /// The sheets' ids in tab order.
    Order(Vec<i64>),
    /// What a shell formula printed.
    Shell { sheet: i64, pos: Pos, run: Run },
}

pub struct Writer {
    tx: Option<Sender<Write>>,
    errors: Receiver<String>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    fn start(conn: Connection) -> Self {
        let (tx, rx) = mpsc::channel::<Write>();
        let (etx, errors) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut conn = conn;
            let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");
            // Everything queued so far goes in one transaction.
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                if let Err(e) = apply(&mut conn, &batch) {
                    let _ = etx.send(format!("Couldn't save: {e}"));
                }
            }
        });
        Self { tx: Some(tx), errors, thread: Some(thread) }
    }

    pub fn send(&self, write: Write) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(write);
        }
    }

    /// A save that failed since the last call, if any.
    pub fn error(&self) -> Option<String> {
        self.errors.try_iter().last()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn apply(conn: &mut Connection, batch: &[Write]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    for w in batch {
        match w {
            Write::Cell { sheet, pos, input } if input.trim().is_empty() => {
                tx.execute(
                    "DELETE FROM shell_results WHERE sheet_id = ?1 AND row = ?2 AND col = ?3",
                    params![sheet, pos.0, pos.1],
                )?;
                tx.execute("DELETE FROM cells WHERE sheet_id = ?1 AND row = ?2 AND col = ?3", params![sheet, pos.0, pos.1])?;
            }
            Write::Cell { sheet, pos, input } => {
                tx.execute(
                    "INSERT INTO cells (sheet_id, row, col, input) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (sheet_id, row, col) DO UPDATE SET input = excluded.input",
                    params![sheet, pos.0, pos.1, input],
                )?;
            }
            Write::AddSheet { id, name } => {
                tx.execute(
                    "INSERT INTO sheets (id, name, position) VALUES (?1, ?2, (SELECT COALESCE(MAX(position) + 1, 0) FROM sheets))",
                    params![id, name],
                )?;
            }
            Write::RenameSheet { id, name } => {
                tx.execute("UPDATE sheets SET name = ?2 WHERE id = ?1", params![id, name])?;
            }
            Write::Shell { sheet, pos, run } => {
                tx.execute(
                    "INSERT INTO shell_results (sheet_id, row, col, formula, status, stdout, stderr, ran_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT (sheet_id, row, col) DO UPDATE SET formula = excluded.formula,
                        status = excluded.status, stdout = excluded.stdout, stderr = excluded.stderr,
                        ran_at = excluded.ran_at",
                    params![sheet, pos.0, pos.1, run.formula, run.status.encode(), run.stdout, run.stderr, run.ran_at],
                )?;
            }
            Write::DeleteSheet { id } => {
                tx.execute("DELETE FROM shell_results WHERE sheet_id = ?1", [id])?;
                tx.execute("DELETE FROM cells WHERE sheet_id = ?1", [id])?;
                tx.execute("DELETE FROM sheets WHERE id = ?1", [id])?;
            }
            Write::Order(ids) => {
                for (i, id) in ids.iter().enumerate() {
                    tx.execute("UPDATE sheets SET position = ?2 WHERE id = ?1", params![id, i as i64])?;
                }
            }
        }
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::Value;

    #[test]
    fn a_new_file_round_trips_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("books.sqlite3");
        let doc = create(&path).unwrap();
        assert!(create(&path).is_err(), "never overwrites");
        doc.writer.send(Write::Cell { sheet: 1, pos: (0, 0), input: "Rent".into() });
        doc.writer.send(Write::Cell { sheet: 1, pos: (0, 1), input: "950".into() });
        doc.writer.send(Write::Cell { sheet: 1, pos: (1, 1), input: "=B1*12".into() });
        doc.writer.send(Write::AddSheet { id: 2, name: "Tax".into() });
        doc.writer.send(Write::Cell { sheet: 2, pos: (3, 3), input: "x".into() });
        doc.writer.send(Write::Cell { sheet: 2, pos: (3, 3), input: "".into() });
        doc.writer.send(Write::RenameSheet { id: 1, name: "Home".into() });
        doc.writer.send(Write::Order(vec![2, 1]));
        drop(doc);

        let doc = open(&path).unwrap();
        assert_eq!(doc.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Tax", "Home"]);
        let home = &doc.sheets[1];
        assert_eq!(home.input((0, 0)), "Rent");
        assert_eq!(home.value((1, 1)), Value::Number(11400.0));
        assert!(doc.sheets[0].is_empty());
        doc.writer.send(Write::DeleteSheet { id: 1 });
        drop(doc);
        assert_eq!(open(&path).unwrap().sheets.len(), 1);
    }

    #[test]
    fn other_databases_are_refused_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.db");
        Connection::open(&path).unwrap().execute_batch("CREATE TABLE t (a);").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(open(&path).err().unwrap().contains("isn't a Ledger file"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(open(&dir.path().join("missing.db")).is_err());
    }

    #[test]
    fn shell_results_are_kept_and_old_files_are_migrated() {
        let dir = tempfile::tempdir().unwrap();
        // A format 1 file, as the first Ledger made it.
        let path = dir.path().join("old.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!("PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = 1;")).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute_batch("INSERT INTO sheets VALUES (1, 'Old', 0); INSERT INTO cells VALUES (1, 0, 0, '=SH(\"echo 7\")');")
                .unwrap();
        }
        let doc = open(&path).unwrap();
        assert!(doc.sheets[0].run((0, 0)).is_none());
        let run = Run {
            formula: "=SH(\"echo 7\")".into(),
            status: Status::Ok,
            stdout: "7\n".into(),
            stderr: String::new(),
            ran_at: 1_000,
        };
        doc.writer.send(Write::Shell { sheet: 1, pos: (0, 0), run: run.clone() });
        drop(doc);
        let doc = open(&path).unwrap();
        assert_eq!(doc.sheets[0].run((0, 0)), Some(&run));
        assert_eq!(doc.sheets[0].value((0, 0)), Value::Number(7.0), "a reload shows the result without running anything");
        doc.writer.send(Write::Cell { sheet: 1, pos: (0, 0), input: String::new() });
        drop(doc);
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM shell_results", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "clearing the cell clears its result");
        let v: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn the_file_is_plain_sql() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("books.sqlite3");
        let doc = create(&path).unwrap();
        doc.writer.send(Write::Cell { sheet: 1, pos: (4, 2), input: "42".into() });
        drop(doc);
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT CAST(input AS INTEGER) FROM cells WHERE row = 4 AND col = 2", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 42);
    }
}
