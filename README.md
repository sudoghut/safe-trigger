# Safe-Trigger

A Rust-based API server that provides a safe and managed way to interact with Large Language Models (LLM). Currently supports Google's Gemini 2.0 Flash model with built-in token management and rate limiting.

## Features

- Token-based API access management
- Automatic rate limiting
- Configurable retry mechanism
- Support for both GET and POST requests
- SQLite-based token storage
- Automatic token rotation and cooldown
- Built-in error handling and recovery

## Prerequisites

- Rust (latest stable version)
- SQLite3
- Google Gemini API key(s)

## Setup

1. Clone the repository:
```bash
git clone https://github.com/yourusername/safe-trigger.git
cd safe-trigger
```

2. Create the SQLite database and table:
```sql
CREATE TABLE TOKENS (
    id INTEGER PRIMARY KEY,
    token TEXT NOT NULL,
    token_type TEXT NOT NULL,
    triggered_on INTEGER,
    delay_by_second INTEGER NOT NULL
);
```

3. Add your Gemini API token(s):
```sql
-- Replace 'your-api-key' with your actual Gemini API key
INSERT INTO TOKENS (token, token_type, delay_by_second) 
VALUES ('your-api-key', 'gemini', 60);
```

4. Build and run the project:
```bash
cargo build
cargo run
```

## API Usage

The server runs on `http://localhost:3000` and provides the following endpoint:

### POST /api/chat

Request body:
```json
{
    "prompt": "Your question or input here",
    "system_prompt": "System instructions for the model"
}
```

### GET /api/chat

Query parameters:
- `prompt`: Your question or input
- `system_prompt`: System instructions for the model

Example:
```
GET /api/chat?prompt=What%20is%20the%20capital%20of%20France?&system_prompt=You%20are%20a%20helpful%20assistant
```

### Response Format

Success response:
```json
{
    "content": "The model's response",
    "token_type": "gemini"
}
```

Error response:
```json
{
    "error": "Error message describing what went wrong"
}
```

## Configuration

The following constants can be adjusted in `src/api_client.rs`:

```rust
pub const MAX_RETRY_ATTEMPTS: u32 = 10;    // Maximum number of retry attempts
pub const RETRY_DELAY_SECONDS: u64 = 30;   // Delay between retries
```

## Token Management

Tokens in the database have the following fields:
- `id`: Unique identifier
- `token`: The API key
- `token_type`: Must be "gemini" for Gemini API
- `triggered_on`: Last usage timestamp
- `delay_by_second`: Cooldown period between uses

The system automatically manages token rotation and respects the cooldown periods.

## Error Handling

The system includes comprehensive error handling for:
- API rate limits
- Network issues
- Invalid tokens
- Malformed requests
- Database errors

When an error occurs, the system will:
1. Automatically retry (up to MAX_RETRY_ATTEMPTS)
2. Wait RETRY_DELAY_SECONDS between attempts
3. Return a descriptive error message if all retries fail

## Current Limitations

- Only supports Google's Gemini 2.0 Flash model
- Token type must be set to "gemini" in the database
- Single server instance (no clustering)
- Local SQLite database (no distributed setup)

## Future Plans

- Support for additional LLM providers
- Distributed token management
- Enhanced rate limiting strategies
- Request caching
- Response streaming
