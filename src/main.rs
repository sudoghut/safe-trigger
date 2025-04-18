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
use api_client::{LLMClient, GeminiClient, LLMError};

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

// Common handler for both GET and POST
async fn handle_chat_request(
    _state: Arc<AppState>,
    request: ChatRequest,
) -> Json<Result<ChatResponse, ErrorResponse>> {
    // Initialize database client
    let log_client = match log_client::DbClient::new() {
        Ok(client) => client,
        Err(e) => return Json(Err(ErrorResponse {
            error: format!("Database connection error: {}", e)
        })),
    };

    // Parse llm parameter as a list of LLMs (token_type)
    let llm_list: Option<Vec<String>> = request.llm.as_ref().map(|s| {
        s.split(',')
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
    });

    // Get a token from the database, filtered by llm if provided
    let token = match &llm_list {
        Some(llms) if !llms.is_empty() => {
            let llm_refs: Vec<&str> = llms.iter().map(|s| s.as_str()).collect();
            match db_client::get_next_token_by_llms(Some(&llm_refs)) {
                Ok(Some(token)) => token,
                Ok(None) => return Json(Err(ErrorResponse {
                    error: "No available tokens for requested LLM(s)".to_string()
                })),
                Err(e) => return Json(Err(ErrorResponse {
                    error: format!("Database error: {}", e)
                })),
            }
        }
        _ => match db_client::get_next_token() {
            Ok(Some(token)) => token,
            Ok(None) => return Json(Err(ErrorResponse {
                error: "No available tokens".to_string()
            })),
            Err(e) => return Json(Err(ErrorResponse {
                error: format!("Database error: {}", e)
            })),
        }
    };

    // Create LLM client based on token type
    let response = match token.token_type.as_str() {
        "gemini" => {
            let client = GeminiClient::new(token.token.clone());
            client.generate_response(&request.prompt, &request.system_prompt, token.id).await
        },
        "openrouter" => {
            // Default model, could be made configurable
            let model = "deepseek/deepseek-chat-v3-0324:free".to_string();
            // let model = "shisa-ai/shisa-v2-llama3.3-70b:free".to_string();
            let client = api_client::OpenRouterClient::new(token.token.clone(), model);
            client.generate_response(&request.prompt, &request.system_prompt, token.id).await
        },
        _ => Err(LLMError("Unsupported token type".to_string())),
    };

    match response {
        Ok(content) => {
            // Log the successful response
            if let Err(e) = log_client.insert_log(
                &request.system_prompt,
                &request.prompt,
                &content,
                &token.token,
                &token.token_type,
            ) {
                println!("Failed to log response: {}", e);
            }
            
            Json(Ok(ChatResponse {
                content,
                token_type: token.token_type,
            }))
        },
        Err(e) => Json(Err(ErrorResponse {
            error: e.to_string(),
        })),
    }
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
    println!("POST to /api/chat with JSON body");
    println!("GET from /api/chat?prompt=...&system_prompt=...");

    // Start the server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
