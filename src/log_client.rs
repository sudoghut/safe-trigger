use rusqlite::{Connection, Result, params};
use chrono::Local;

pub struct DbClient {
    db_path: String,
}

impl DbClient {
    // new now takes the path and just stores it. It also ensures the table exists.
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS LOGS (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                system_prompt TEXT NOT NULL,
                prompt TEXT NOT NULL,
                response TEXT NOT NULL,
                token TEXT NOT NULL,
                token_type TEXT NOT NULL,
                time TEXT NOT NULL
            )",
            [],
        )?;
        Ok(Self { db_path: db_path.to_string() })
    }

    // insert_log now opens its own connection
    pub fn insert_log(
        &self, // Keep &self for consistency, though db_path could be passed directly
        system_prompt: &str,
        prompt: &str,
        response: &str,
        token: &str,
        token_type: &str,
    ) -> Result<()> {
        // Try to open the connection
        let conn = match Connection::open(&self.db_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("CRITICAL: Failed to OPEN log database connection at '{}': {}", self.db_path, e);
                return Err(e); // Propagate the error
            }
        };

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // Try to execute the insert statement
        match conn.execute(
            "INSERT INTO LOGS (system_prompt, prompt, response, token, token_type, time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![system_prompt, prompt, response, token, token_type, now],
        ) {
            Ok(_) => Ok(()), // Success
            Err(e) => {
                eprintln!("CRITICAL: Failed to EXECUTE insert into LOGS table: {}", e);
                Err(e) // Propagate the error
            }
        }
        // Connection is dropped here automatically
    }
}
