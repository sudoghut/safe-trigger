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

#[async_trait::async_trait]
pub trait LLMClient {
    async fn generate_response(&self, prompt: &str, system_prompt: &str) -> Result<String, LLMError>;
}

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
}

#[async_trait::async_trait]
impl LLMClient for GeminiClient {
    async fn generate_response(&self, prompt: &str, system_prompt: &str) -> Result<String, LLMError> {
        let mut attempts = 0;
        
        loop {
            match self.attempt_generate(prompt, system_prompt).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    attempts += 1;
                    if attempts >= MAX_RETRY_ATTEMPTS {
                        return Err(LLMError(format!("Max retry attempts ({}) reached. Last error: {}", MAX_RETRY_ATTEMPTS, e)));
                    }
                    println!("Attempt {} failed: {}. Retrying in {} seconds...", attempts, e, RETRY_DELAY_SECONDS);
                    sleep(Duration::from_secs(RETRY_DELAY_SECONDS)).await;
                }
            }
        }
    }
}
