use rusqlite::{Connection, Result, OptionalExtension, params};
use chrono::Utc;

pub struct Token {
    pub id: i64,
    pub token: String,
    pub token_type: String,
    triggered_on: Option<i64>,
    delay_by_second: i64,
}

pub fn get_next_token() -> Result<Option<Token>> {
    let conn = Connection::open("data.db")?;
    
    // Get current timestamp
    let current_time = Utc::now().timestamp();
    
    // Select a random token where either:
    // 1. triggered_on is NULL, or
    // 2. triggered_on + delay_by_second is before current time
    let mut stmt = conn.prepare("
        SELECT id, token, token_type, triggered_on, delay_by_second 
        FROM TOKENS 
        WHERE triggered_on IS NULL 
        OR (triggered_on + delay_by_second) < ?
        ORDER BY RANDOM()
        LIMIT 1
    ")?;

    let token = stmt.query_row(params![current_time], |row| {
        Ok(Token {
            id: row.get(0)?,
            token: row.get(1)?,
            token_type: row.get(2)?,
            triggered_on: row.get(3)?,
            delay_by_second: row.get(4)?,
        })
    }).optional()?;

    // If we found a token, update its triggered_on timestamp
    if let Some(token) = &token {
        conn.execute(
            "UPDATE TOKENS SET triggered_on = ? WHERE id = ?",
            params![current_time, token.id],
        )?;
    }
    
    
    Ok(token)
}
