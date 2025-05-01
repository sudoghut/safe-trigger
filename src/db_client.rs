use rusqlite::{Connection, Result, OptionalExtension, params};
use chrono::Utc;

pub struct Token {
    pub id: i64,
    pub token: String,
    pub token_type: String,
}

/// Get next token, optionally filtered by a list of LLM names (token_type).
pub fn get_next_token_by_llms(llms: Option<&[&str]>) -> Result<Option<Token>> {
    let conn = Connection::open("data.db")?;
    let current_time = Utc::now().timestamp();

    let (sql, params): (String, Vec<rusqlite::types::Value>) = if let Some(llms) = llms {
        if llms.is_empty() {
            (
                "
                SELECT id, token, token_type, triggered_on, delay_by_second, trouble_delay 
                FROM TOKENS 
                WHERE triggered_on IS NULL 
                OR (triggered_on + delay_by_second) < ?
                ORDER BY triggered_on ASC
                LIMIT 1
                ".to_string(),
                vec![current_time.into()],
            )
        } else {
            let placeholders = llms.iter().map(|_| "?".to_string()).collect::<Vec<_>>().join(",");
            let sql = format!(
                "
                SELECT id, token, token_type, triggered_on, delay_by_second, trouble_delay 
                FROM TOKENS 
                WHERE (triggered_on IS NULL OR (triggered_on + delay_by_second) < ?)
                AND token_type IN ({})
                ORDER BY triggered_on ASC
                LIMIT 1
                ",
                placeholders
            );
            let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(1 + llms.len());
            params.push(current_time.into());
            for llm in llms {
                params.push(llm.to_string().into());
            }
            (sql, params)
        }
    } else {
        (
            "
            SELECT id, token, token_type, triggered_on, delay_by_second, trouble_delay 
            FROM TOKENS 
            WHERE triggered_on IS NULL 
            OR (triggered_on + delay_by_second) < ?
            ORDER BY triggered_on ASC
            LIMIT 1
            ".to_string(),
            vec![current_time.into()],
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let token = stmt.query_row(rusqlite::params_from_iter(params.iter()), |row| {
        Ok(Token {
            id: row.get(0)?,
            token: row.get(1)?,
            token_type: row.get(2)?,
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

pub fn mark_token_trouble(token_id: i64) -> Result<()> {
    let conn = Connection::open("data.db")?;
    
    // Update trouble_delay to 1 and add 1 hour to delay_by_second
    conn.execute(
        "UPDATE TOKENS SET 
        trouble_delay = 1, 
        delay_by_second = delay_by_second + 3600 
        WHERE id = ?",
        params![token_id],
    )?;
    
    Ok(())
}

// Function to get token details by ID
pub fn get_token_by_id(token_id: i64) -> Result<Option<Token>> {
    let conn = Connection::open("data.db")?;
    let mut stmt = conn.prepare("SELECT id, token, token_type FROM TOKENS WHERE id = ?")?;
    let token = stmt.query_row(params![token_id], |row| {
        Ok(Token {
            id: row.get(0)?,
            token: row.get(1)?,
            token_type: row.get(2)?,
        })
    }).optional()?;
    Ok(token)
}

// Function to check if a token is marked as in trouble
pub fn is_token_in_trouble(token_id: i64) -> Result<bool> {
    let conn = Connection::open("data.db")?;
    let mut stmt = conn.prepare("SELECT trouble_delay FROM TOKENS WHERE id = ?")?;
    
    let result = stmt.query_row(params![token_id], |row| {
        let trouble_delay: i8 = row.get(0)?; // Assuming trouble_delay is stored as INTEGER (compatible with i8)
        Ok(trouble_delay == 1)
    });

    match result {
        Ok(is_troubled) => Ok(is_troubled),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false), // Token not found is not considered "in trouble"
        Err(e) => Err(e), // Propagate other database errors
    }
}

pub fn clear_token_trouble(token_id: i64) -> Result<()> {
    let conn = Connection::open("data.db")?;

    // First, check if the token has trouble_delay = 1
    let mut stmt = conn.prepare("SELECT trouble_delay FROM TOKENS WHERE id = ?")?;
    let trouble_delay: i8 = stmt.query_row(params![token_id], |row| row.get(0))?;

    // Only update if trouble_delay is 1
    if trouble_delay == 1 {
        conn.execute(
            "UPDATE TOKENS SET 
            trouble_delay = 0, 
            delay_by_second = MAX(0, delay_by_second - 3600) 
            WHERE id = ?",
            params![token_id],
        )?;
    }
    
    Ok(())
}
