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

// Configuration constants
pub const MAX_RETRY_ATTEMPTS: u32 = 10;
pub const RETRY_DELAY_SECONDS: u64 = 30;

// Response from API attempt containing both result and used token info
pub struct AttemptResult {
    pub result: Result<String, LLMError>
}

#[async_trait::async_trait]
pub trait LLMClient {
    async fn generate_response(&self, prompt: &str, system_prompt: &str, initial_token_id: i64) -> Result<String, LLMError>;
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
            // print!("Response: {:?}", response);
            let response_json: Value = response.json()
                .await
                .map_err(|e| LLMError(e.to_string()))?;
            // Extract the assistant's reply from the response
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
            // print!("Response: {:?}", response);
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
impl LLMClient for OpenRouterClient {
    async fn generate_response(&self, prompt: &str, system_prompt: &str, initial_token_id: i64) -> Result<String, LLMError> {
        let mut attempts = 0;
        let mut current_token_id = initial_token_id;
        let mut current_client = self.clone();

        loop {
            let attempt_result = current_client.attempt_with_token(prompt, system_prompt).await;

            match attempt_result.result {
                Ok(response) => {
                    if let Err(e) = crate::db_client::clear_token_trouble(current_token_id) {
                        println!("Warning: Failed to clear token trouble status: {}", e);
                    }
                    return Ok(response)
                },
                Err(e) => {
                    attempts += 1;

                    if let Err(db_err) = crate::db_client::mark_token_trouble(current_token_id) {
                        println!("Warning: Failed to mark token as troubled: {}", db_err);
                    }

                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(LLMError(format!("Max retry attempts ({}) reached. Last error: {}", MAX_RETRY_ATTEMPTS, e)));
                    }

                    // Try to get a new token for openrouter
                    match crate::db_client::get_next_token_by_llms(Some(&["openrouter"])) {
                        Ok(Some(new_token)) => {
                            println!("Attempt {} failed: {}. Using new token for retry in {} seconds...", attempts, e, RETRY_DELAY_SECONDS);
                            current_token_id = new_token.id;
                            // Default model, could be made configurable
                            current_client = OpenRouterClient::new(new_token.token, self.model.clone());
                            sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                        },
                        Ok(None) => {
                            return Err(LLMError("No available tokens for retry".to_string()));
                        },
                        Err(db_err) => {
                            return Err(LLMError(format!("Failed to get new token for retry: {}", db_err)));
                        }
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
        let model_id = "gemini-2.0-flash";
        let generate_content_api = "streamGenerateContent";
        
        let request_body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "text": prompt
                        }
                    ]
                }
            ],
            "systemInstruction": {
                "parts": [
                    {
                        "text": system_prompt
                    }
                ]
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
            let response_text = response.text()
                .await
                .map_err(|e| LLMError(e.to_string()))?;
            let response_array: Vec<Value> = serde_json::from_str(&response_text)
                .map_err(|e| 
                    LLMError(format!("Failed to parse response: {}, Raw response: {}", e, response_text))
                )?;
            
            let full_response: String = response_array.iter()
                .filter_map(|chunk| {
                    chunk.get("candidates")?.get(0)?
                        .get("content")?.get("parts")?.get(0)?
                        .get("text")?.as_str()
                })
                .collect();
            
            Ok(full_response)
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
    async fn generate_response(&self, prompt: &str, system_prompt: &str, initial_token_id: i64) -> Result<String, LLMError> {
        let mut attempts = 0;
        let mut current_token_id = initial_token_id;
        let mut current_client = self.clone();
        
        loop {
            let attempt_result = current_client.attempt_with_token(prompt, system_prompt).await;
            
            match attempt_result.result {
                Ok(response) => {
                    // If successful and token had trouble_delay = 1, clear it
                    if let Err(e) = crate::db_client::clear_token_trouble(current_token_id) {
                        println!("Warning: Failed to clear token trouble status: {}", e);
                    }
                    return Ok(response)
                },
                Err(e) => {
                    attempts += 1;
                    
                    // Mark current token as troubled
                    if let Err(db_err) = crate::db_client::mark_token_trouble(current_token_id) {
                        println!("Warning: Failed to mark token as troubled: {}", db_err);
                    }

                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(LLMError(format!("Max retry attempts ({}) reached. Last error: {}", MAX_RETRY_ATTEMPTS, e)));
                    }

                    // Try to get a new token
                    match crate::db_client::get_next_token() {
                        Ok(Some(new_token)) => {
                            println!("Attempt {} failed: {}. Using new token for retry in {} seconds...", attempts, e, RETRY_DELAY_SECONDS);
                            current_token_id = new_token.id;
                            current_client = GeminiClient::new(new_token.token);
                            sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                        },
                        Ok(None) => {
                            return Err(LLMError("No available tokens for retry".to_string()));
                        },
                        Err(db_err) => {
                            return Err(LLMError(format!("Failed to get new token for retry: {}", db_err)));
                        }
                    }
                }
            }
        }
    }
}
