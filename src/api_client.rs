use crate::db_client;
use crate::log_client;
use serde_json::{json, Value};
use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;
use tokio::time::sleep;

// Custom error type that implements Send + Sync
#[derive(Debug)]
pub struct LLMError(pub String);

impl fmt::Display for LLMError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StdError for LLMError {}

impl From<reqwest::Error> for LLMError {
    fn from(err: reqwest::Error) -> Self {
        LLMError(err.to_string())
    }
}

impl From<&str> for LLMError {
    fn from(s: &str) -> Self {
        LLMError(s.to_string())
    }
}

impl From<String> for LLMError {
    fn from(s: String) -> Self {
        LLMError(s)
    }
}

// Add conversion from rusqlite::Error
impl From<rusqlite::Error> for LLMError {
    fn from(err: rusqlite::Error) -> Self {
        LLMError(format!("Database error: {}", err))
    }
}

// Configuration constants
pub const MAX_RETRY_ATTEMPTS: u32 = 10;
pub const RETRY_DELAY_SECONDS: u64 = 30;

// Response from API attempt containing both result and used token info
pub struct AttemptResult {
    pub result: Result<String, LLMError>
}

#[async_trait::async_trait]
pub trait LLMClient {
    async fn generate_response(
        &self,
        prompt: &str,
        system_prompt: &str,
        initial_token_id: i64,
        log_db: &log_client::DbClient, // Add log client
        llm_conditions: Option<&[&str]>, // Add LLM conditions for retry
    ) -> Result<String, LLMError>;
}

#[derive(Clone)]
pub struct OpenRouterClient {
    api_key: String,
    model: String,
}

impl OpenRouterClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self { api_key, model }
    }

    async fn attempt_generate(&self, prompt: &str, system_prompt: &str) -> Result<String, LLMError> {
        let request_body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        });

        let api_url = "https://openrouter.ai/api/v1/chat/completions";
        let client = reqwest::Client::new();
        let response = client
            .post(api_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| LLMError(e.to_string()))?;

        if response.status().is_success() {
            let response_json: Value = response.json()
                .await
                .map_err(|e| LLMError(e.to_string()))?;
            if let Some(choices) = response_json.get("choices") {
                if let Some(choice) = choices.get(0) {
                    if let Some(message) = choice.get("message") {
                        if let Some(content) = message.get("content") {
                            if let Some(text) = content.as_str() {
                                return Ok(text.to_string());
                            }
                        }
                    }
                }
            }
            Err(LLMError("Failed to parse OpenRouter response".to_string()))
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|e| e.to_string());
            Err(LLMError(format!("Error: {} - {}", status, error_text)))
        }
    }

    async fn attempt_with_token(&self, prompt: &str, system_prompt: &str) -> AttemptResult {
        let result = self.attempt_generate(prompt, system_prompt).await;
        AttemptResult { result }
    }
}

// Helper function to handle the retry logic
async fn handle_retry(
    attempts: &mut u32,
    current_token_id: i64,
    current_token_type: &str, // Needed for logging
    current_token_value: &str, // Needed for logging
    prompt: &str,
    system_prompt: &str,
    error: &LLMError,
    log_db: &log_client::DbClient,
    llm_conditions: Option<&[&str]>,
) -> Result<Option<(i64, String, String)>, LLMError> { // Returns Option<(id, value, type)> or fatal Error
    *attempts += 1;

    if *attempts >= MAX_RETRY_ATTEMPTS {
        return Err(LLMError(format!(
            "Max retry attempts ({}) reached. Last error on token {}: {}",
            MAX_RETRY_ATTEMPTS, current_token_id, error
        )));
    }

    if let Err(log_err) = log_db.insert_log(
        system_prompt,
        prompt,
        &error.to_string(),
        current_token_value,
        current_token_type,
    ) {
        // Use eprintln for errors and make the message more prominent
        eprintln!("CRITICAL WARNING: FAILED TO LOG ERROR TO DATABASE (data.db): {}", log_err);
    }

    if let Err(db_err) = db_client::mark_token_trouble(current_token_id) {
        println!("Warning: Failed to mark token {} as troubled: {}", current_token_id, db_err);
    }

    match db_client::get_next_token_by_llms(llm_conditions) {
        Ok(Some(new_token)) => {
            println!(
                "Attempt {} failed for token {}: {}. Using new token {} ({}) for retry in {} seconds...",
                *attempts, current_token_id, error, new_token.id, new_token.token_type, RETRY_DELAY_SECONDS
            );
            Ok(Some((new_token.id, new_token.token, new_token.token_type)))
        }
        Ok(None) => {
             println!(
                "Attempt {} failed for token {}: {}. No other suitable tokens found this time. Retrying after delay...",
                *attempts, current_token_id, error
            );
             Ok(None)
        }
        Err(db_err) => {
             Err(LLMError(format!(
                "Failed to get new token for retry after error on token {}: {}",
                current_token_id, db_err
            )))
        }
    }
}

#[async_trait::async_trait]
impl LLMClient for OpenRouterClient {
    async fn generate_response(
        &self,
        prompt: &str,
        system_prompt: &str,
        initial_token_id: i64,
        log_db: &log_client::DbClient,
        llm_conditions: Option<&[&str]>,
    ) -> Result<String, LLMError> {
        let mut attempts = 0;
        let mut current_token_id = initial_token_id;

        let initial_token_details = db_client::get_token_by_id(current_token_id)
            .map_err(LLMError::from)?
            .ok_or_else(|| LLMError(format!("Initial token ID {} not found", current_token_id)))?;

        if initial_token_details.token_type != "openrouter" {
            return Err(LLMError(format!(
                "Initial token {} is type '{}', expected 'openrouter'",
                current_token_id, initial_token_details.token_type
            )));
        }

        let mut current_client = OpenRouterClient::new(initial_token_details.token.clone(), self.model.clone());
        let mut current_token_type = initial_token_details.token_type.clone();
        let mut current_token_value = initial_token_details.token.clone();

        loop {
            let attempt_result = current_client.attempt_with_token(prompt, system_prompt).await;

            match attempt_result.result {
                Ok(response) => {
                    if let Err(log_err) = log_db.insert_log(
                        system_prompt, prompt, &response, &current_token_value, &current_token_type,
                    ) {
                        println!("Warning: Failed to log success: {}", log_err);
                    }
                    if let Err(e) = db_client::clear_token_trouble(current_token_id) {
                        println!("Warning: Failed to clear token trouble status for {}: {}", current_token_id, e);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    match handle_retry(
                        &mut attempts, current_token_id, &current_token_type, &current_token_value,
                        prompt, system_prompt, &e, log_db, llm_conditions,
                    ).await {
                        Ok(Some((new_id, new_token, new_type))) => {
                            current_token_id = new_id;
                            current_token_value = new_token.clone();
                            current_token_type = new_type.clone();

                            if current_token_type == "openrouter" {
                                current_client = OpenRouterClient::new(current_token_value.clone(), self.model.clone());
                                println!("Retrying with new OpenRouter token ID: {}", current_token_id);
                            } else {
                                println!(
                                    "Token type changed from 'openrouter' to '{}' (ID: {}). Cannot continue with OpenRouterClient.",
                                    current_token_type, current_token_id
                                );
                                return Err(LLMError(format!(
                                    "Token type switched to '{}' (ID: {}), requires different client. Last error: {}",
                                    current_token_type, current_token_id, e
                                )));
                            }
                        }
                        Ok(None) => {
                            println!("No suitable token found, sleeping before retry...");
                            sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                            continue;
                        }
                        Err(retry_err) => return Err(retry_err),
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct GeminiClient {
    api_key: String,
}

impl GeminiClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    async fn attempt_generate(&self, prompt: &str, system_prompt: &str) -> Result<String, LLMError> {
        let model_id = "gemini-1.5-flash"; // Corrected model ID if needed, or keep as 2.0
        let generate_content_api = "generateContent"; // Use generateContent for non-streaming

        let request_body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [ { "text": prompt } ]
                }
            ],
            "systemInstruction": {
                "parts": [ { "text": system_prompt } ]
            },
            "generationConfig": {
                "responseMimeType": "text/plain"
            }
        });

        let api_url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:{}?key={}",
            model_id, generate_content_api, self.api_key
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&api_url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| LLMError(e.to_string()))?;

        if response.status().is_success() {
            let response_json: Value = response.json()
                .await
                .map_err(|e| LLMError(format!("Failed to parse JSON response: {}", e)))?;

            // Adjusted parsing for non-streaming generateContent response
            if let Some(candidates) = response_json.get("candidates") {
                 if let Some(candidate) = candidates.get(0) {
                     if let Some(content) = candidate.get("content") {
                         if let Some(parts) = content.get("parts") {
                             if let Some(part) = parts.get(0) {
                                 if let Some(text) = part.get("text") {
                                     if let Some(text_str) = text.as_str() {
                                         return Ok(text_str.to_string());
                                     }
                                 }
                             }
                         }
                     }
                 }
            }
             Err(LLMError(format!("Failed to extract text from Gemini response: {:?}", response_json)))

        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|e| e.to_string());
            Err(LLMError(format!("Error: {} - {}", status, error_text)))
        }
    }


    async fn attempt_with_token(&self, prompt: &str, system_prompt: &str) -> AttemptResult {
        let result = self.attempt_generate(prompt, system_prompt).await;
        AttemptResult { result }
    }
}

#[async_trait::async_trait]
impl LLMClient for GeminiClient {
    async fn generate_response(
        &self,
        prompt: &str,
        system_prompt: &str,
        initial_token_id: i64,
        log_db: &log_client::DbClient,
        llm_conditions: Option<&[&str]>,
    ) -> Result<String, LLMError> {
        let mut attempts = 0;
        let mut current_token_id = initial_token_id;

        let initial_token_details = db_client::get_token_by_id(current_token_id)
            .map_err(LLMError::from)?
            .ok_or_else(|| LLMError(format!("Initial token ID {} not found", current_token_id)))?;

        if initial_token_details.token_type != "gemini" {
             return Err(LLMError(format!(
                "Initial token {} is type '{}', expected 'gemini'",
                current_token_id, initial_token_details.token_type
            )));
        }

        let mut current_client = GeminiClient::new(initial_token_details.token.clone());
        let mut current_token_type = initial_token_details.token_type.clone();
        let mut current_token_value = initial_token_details.token.clone();

        loop {
            let attempt_result = current_client.attempt_with_token(prompt, system_prompt).await;

            match attempt_result.result {
                Ok(response) => {
                    if let Err(log_err) = log_db.insert_log(
                        system_prompt, prompt, &response, &current_token_value, &current_token_type,
                    ) {
                        println!("Warning: Failed to log success: {}", log_err);
                    }
                    if let Err(e) = db_client::clear_token_trouble(current_token_id) {
                        println!("Warning: Failed to clear token trouble status for {}: {}", current_token_id, e);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    match handle_retry(
                        &mut attempts, current_token_id, &current_token_type, &current_token_value,
                        prompt, system_prompt, &e, log_db, llm_conditions,
                    ).await {
                        Ok(Some((new_id, new_token, new_type))) => {
                            current_token_id = new_id;
                            current_token_value = new_token.clone();
                            current_token_type = new_type.clone();

                            if current_token_type == "gemini" {
                                current_client = GeminiClient::new(current_token_value.clone());
                                println!("Retrying with new Gemini token ID: {}", current_token_id);
                            } else {
                                println!(
                                    "Token type changed from 'gemini' to '{}' (ID: {}). Cannot continue with GeminiClient.",
                                    current_token_type, current_token_id
                                );
                                return Err(LLMError(format!(
                                    "Token type switched to '{}' (ID: {}), requires different client. Last error: {}",
                                    current_token_type, current_token_id, e
                                )));
                            }
                        }
                        Ok(None) => {
                            println!("No suitable token found, sleeping before retry...");
                            sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                            continue;
                        }
                        Err(retry_err) => return Err(retry_err),
                    }
                }
            }
        }
    }
}
