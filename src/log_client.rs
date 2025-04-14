use rusqlite::{Connection, Result};
use chrono::Local;

pub struct DbClient {
    conn: Connection,
}

impl DbClient {
    pub fn new() -> Result<Self> {
        let conn = Connection::open("data.db")?;
        
        // Create table if it doesn't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS LOGS (
                id INTEGER PRIMARY KEY,
                system_prompt TEXT NOT NULL,
                prompt TEXT NOT NULL,
                response TEXT NOT NULL,
                token TEXT NOT NULL,
                token_type TEXT NOT NULL,
                time TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn insert_log(
        &self,
        system_prompt: &str,
        prompt: &str,
        response: &str,
        token: &str,
        token_type: &str,
    ) -> Result<()> {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        
        self.conn.execute(
            "INSERT INTO LOGS (system_prompt, prompt, response, token, token_type, time) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (system_prompt, prompt, response, token, token_type, now),
        )?;

        Ok(())
    }
}
