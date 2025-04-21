mod db_client;
mod api_client;
mod log_client;

use axum::{
    extract::{Json, Query, State},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use api_client::{LLMClient, GeminiClient, OpenRouterClient, LLMError}; // Added OpenRouterClient here
use regex::Regex; // Import Regex

#[derive(Deserialize)]
struct ChatRequest {
    prompt: String,
    system_prompt: String,
    llm: Option<String>, // Comma-separated list of LLMs, e.g. "gemini,openrouter"
}

// Define the response structure
#[derive(Serialize)]
struct ChatResponse {
    content: String,
    token_type: String,
}

// Error response
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// Empty app state since we create clients per-request
struct AppState {}

// Handler for POST requests
async fn handle_post_chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Json<Result<ChatResponse, ErrorResponse>> {
    handle_chat_request(state, request).await
}

// Handler for GET requests
async fn handle_get_chat(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ChatRequest>,
) -> Json<Result<ChatResponse, ErrorResponse>> {
    handle_chat_request(state, params).await
}

// Helper function to parse the token ID from the switch error message
fn parse_token_id_from_switch_error(error_msg: &str) -> Option<i64> {
    // Example error: "Token type switched to 'gemini' (ID: 5), requires different client. Last error: ..."
    let re = Regex::new(r"\(ID: (\d+)\)").unwrap(); // Simple regex to find (ID: number)
    re.captures(error_msg)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse::<i64>().ok())
}


// Common handler for both GET and POST
async fn handle_chat_request(
    _state: Arc<AppState>,
    request: ChatRequest,
) -> Json<Result<ChatResponse, ErrorResponse>> {
    // Initialize log database client
    let log_client = match log_client::DbClient::new("data.db") { // Ensure path is correct
        Ok(client) => client,
        Err(e) => return Json(Err(ErrorResponse {
            error: format!("Log database connection error: {}", e)
        })),
    };

    // Parse llm parameter
    let llm_list: Option<Vec<String>> = request.llm.as_ref().map(|s| {
        s.split(',')
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
    });

    let llm_conditions_vec: Option<Vec<&str>> = llm_list.as_ref().map(|llms| {
        llms.iter().map(|s| s.as_str()).collect()
    });
    let llm_conditions_slice: Option<&[&str]> = llm_conditions_vec.as_deref();

    // Get the initial token
    let mut current_token = match db_client::get_next_token_by_llms(llm_conditions_slice) {
        Ok(Some(token)) => token,
        Ok(None) => {
            let error_msg = if let Some(conds) = llm_conditions_slice {
                format!("No available tokens matching conditions: {:?}", conds)
            } else {
                "No available tokens".to_string()
            };
            return Json(Err(ErrorResponse { error: error_msg }));
        }
        Err(e) => return Json(Err(ErrorResponse {
            error: format!("Database error getting initial token: {}", e)
        })),
    };

    // Loop to handle potential client switches
    loop {
        let response_result = match current_token.token_type.as_str() {
            "gemini" => {
                println!("Using Gemini client with token ID: {}", current_token.id);
                let client = GeminiClient::new(current_token.token.clone());
                client.generate_response(&request.prompt, &request.system_prompt, current_token.id, &log_client, llm_conditions_slice).await
            },
            "openrouter" => {
                 println!("Using OpenRouter client with token ID: {}", current_token.id);
                // Default model, could be made configurable
                let model = "deepseek/deepseek-chat".to_string(); // Example model
                let client = OpenRouterClient::new(current_token.token.clone(), model);
                client.generate_response(&request.prompt, &request.system_prompt, current_token.id, &log_client, llm_conditions_slice).await
            },
            unsupported_type => {
                 println!("Encountered unsupported token type: {}", unsupported_type);
                 Err(LLMError(format!("Unsupported token type '{}' for token ID {}", unsupported_type, current_token.id)))
            }
        };

        match response_result {
            Ok(content) => {
                // Successful response, break the loop and return
                return Json(Ok(ChatResponse {
                    content,
                    token_type: current_token.token_type, // Return the type of the token that succeeded
                }));
            }
            Err(e) => {
                let error_string = e.to_string();
                // Check if it's the specific error indicating a client switch is needed
                if error_string.contains("requires different client") {
                     println!("Detected token type switch requirement: {}", error_string);
                    // Attempt to parse the new token ID from the error message
                    if let Some(new_token_id) = parse_token_id_from_switch_error(&error_string) {
                         println!("Attempting to switch to token ID: {}", new_token_id);
                        // Fetch the details of the new token
                        match db_client::get_token_by_id(new_token_id) {
                            Ok(Some(new_token_details)) => {
                                 println!("Successfully fetched details for new token ID: {}", new_token_id);
                                current_token = new_token_details; // Update current_token
                                continue; // Continue the loop to try with the new client/token
                            }
                            Ok(None) => {
                                 println!("Failed to find details for switched token ID: {}", new_token_id);
                                // If the new token ID isn't found, return an error
                                return Json(Err(ErrorResponse {
                                    error: format!("Failed to switch client: New token ID {} not found after error: {}", new_token_id, error_string)
                                }));
                            }
                            Err(db_err) => {
                                 println!("Database error fetching details for switched token ID {}: {}", new_token_id, db_err);
                                // If there's a DB error fetching the new token, return an error
                                return Json(Err(ErrorResponse {
                                    error: format!("Database error fetching switched token ID {}: {}. Original error: {}", new_token_id, db_err, error_string)
                                }));
                            }
                        }
                    } else {
                         println!("Failed to parse new token ID from switch error message: {}", error_string);
                        // If we couldn't parse the ID from the error, return the original error
                        return Json(Err(ErrorResponse { error: error_string }));
                    }
                } else {
                    // Any other error (max retries, initial unsupported type, DB error during retry, etc.)
                     println!("Non-switch error encountered: {}", error_string);
                    return Json(Err(ErrorResponse { error: error_string }));
                }
            }
        }
    } // End loop
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize empty app state
    let state = Arc::new(AppState {});

    // Create the router with both GET and POST endpoints
    let app = Router::new()
        .route("/api/chat", post(handle_post_chat))
        .route("/api/chat", get(handle_get_chat))
        .with_state(state);

    // Set up the server address
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server listening on {}", addr);
    println!("POST to /api/chat with JSON body {{ \"prompt\": \"...\", \"system_prompt\": \"...\", \"llm\": \"optional,comma,separated\" }}");
    println!("GET from /api/chat?prompt=...&system_prompt=...&llm=optional,comma,separated");


    // Start the server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
