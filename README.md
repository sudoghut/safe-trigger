# Safe-Trigger

A Rust-based API server providing managed access to Large Language Models (LLMs) like Google Gemini and OpenRouter. It features built-in token management, rate limiting, and automatic retries.

## Features

-   **LLM Support:** Google Gemini and OpenRouter.
-   **Token Management:** Rotates LLM API keys stored in an SQLite database, respecting cooldown periods.
-   **Rate Limiting:** Prevents exceeding API limits through token cooldowns.
-   **Access Control:** Optional server-level access token for added security.
-   **API Interface:** Supports both GET and POST requests to `/api/chat`.
-   **Error Handling:** Gracefully handles API errors, network issues, and database problems.

## Prerequisites

-   Rust (latest stable version recommended)
-   SQLite3
-   API Keys: Google Gemini and/or OpenRouter API keys.

## Setup & Running

1.  **Clone the Repository:**
    ```bash
    git clone https://github.com/yourusername/safe-trigger.git # Replace with your repo URL if forked
    cd safe-trigger
    ```

2.  **Initialize Database:**
    Create an SQLite database file (e.g., `data.db`) and run the following SQL command to create the necessary table:
    ```sql
    -- Using sqlite3 command-line tool:
    -- sqlite3 data.db < schema.sql
    -- Or manually:
    CREATE TABLE TOKENS (
        id INTEGER PRIMARY KEY,
        token TEXT NOT NULL,          -- The LLM API Key
        token_type TEXT NOT NULL,     -- 'gemini' or 'openrouter'
        triggered_on INTEGER,         -- Timestamp of last use (Unix epoch)
        delay_by_second INTEGER NOT NULL -- Cooldown period in seconds
    );
    ```
    *(Note: `data.db` is ignored by default in `.gitignore`)*

3.  **Add LLM API Keys:**
    Insert your API keys into the `TOKENS` table. Set `token_type` to either `gemini` or `openrouter` and specify a `delay_by_second` cooldown (e.g., 30 seconds).
    ```sql
    -- Example for Gemini:
    INSERT INTO TOKENS (token, token_type, delay_by_second)
    VALUES ('YOUR_GEMINI_API_KEY', 'gemini', 30);

    -- Example for OpenRouter:
    INSERT INTO TOKENS (token, token_type, delay_by_second)
    VALUES ('YOUR_OPENROUTER_API_KEY', 'openrouter', 30);
    ```

4.  **Configure Server Access Token (Optional):**
    For an extra layer of security (to control who can use *this server*), you can set a server access token:
    a.  Rename `_access_token.txt` to `access_token.txt`.
    b.  Edit `access_token.txt` and place your desired secret token (password) on the first line.
    c.  If this file contains a token, all API requests must include a matching `access_token` parameter/field (see API Usage).
    d.  If `access_token.txt` is empty or doesn't exist, this check is skipped.
    *(Note: `access_token.txt` is ignored by default in `.gitignore`)*

5.  **Build and Run:**
    ```bash
    cargo build --release
    ./target/release/safe-trigger
    # Or using cargo run (for development):
    # cargo run
    ```
    The server will start listening on `0.0.0.0:3000`.

## Building with Docker (Alternative)

You can build a release binary within a Fedora Docker container:

1.  **Build Image:** `docker build -t safe-trigger-fedora .`
2.  **Create Container:** `docker create --name safe-trigger-fedora-container safe-trigger-fedora`
3.  **Copy Binary:** `docker cp safe-trigger-fedora-container:/app/target/release/safe-trigger .`
4.  **Cleanup (Optional):** `docker rm safe-trigger-fedora-container`
5.  **Remove Image (Optional):** `docker rmi safe-trigger-fedora`

Now you have the `safe-trigger` binary built in your host server, ready to run on a compatible system.


## Running as a Systemd Service (Fedora)

To run `safe-trigger` as a background service managed by `systemd` on Fedora:

1.  **Prerequisites:**
    *   Ensure you have built the release binary (`./target/release/safe-trigger`).
    *   Make the binary executable: `sudo chmod +x ./target/release/safe-trigger`
    *   Place the `safe-trigger` project directory (containing the binary, `data.db`, `access_token.txt`, etc.) in a stable location (e.g., `/opt/safe-trigger` or `/srv/safe-trigger`). Avoid using user home directories if the service needs to run system-wide or survive user logouts.
    *   Decide which user and group the service should run as. It's recommended to create a dedicated user (e.g., `safe-trigger-user`) for security rather than running as root.

2.  **Create a systemd Unit File:**
    Create a file named `safe-trigger.service` in `/etc/systemd/system/` with the following content. **Change the following values** for your actual settings.

    ```ini
    [Unit]
    Description=Safe-Trigger LLM API Server
    After=network.target

    [Service]
    User=linuxuser
    Group=linuxuser
    WorkingDirectory=/home/linuxuser/safe-trigger
    ExecStart=/usr/bin/env /home/linuxuser/safe-trigger/safe-trigger
    Restart=on-failure
    StandardOutput=journal
    StandardError=journal

    [Install]
    WantedBy=multi-user.target
    ```

    *   `User`/`Group`: The user/group the service will run under. Ensure this user has read/write permissions for the `WorkingDirectory`, `data.db`, and `access_token.txt`.
    *   `WorkingDirectory`: The absolute path to the directory where you placed the `safe-trigger` project.
    *   `ExecStart`: The absolute path to the compiled `safe-trigger` binary.

3.  **Enable and Start the Service:**
    ```bash
    # Reload systemd to recognize the new service file
    sudo systemctl daemon-reload

    # Enable the service to start on boot
    sudo systemctl enable safe-trigger.service

    # Start the service immediately
    sudo systemctl start safe-trigger.service
    ```

4.  **Manage the Service:**
    *   **Check Status:** `sudo systemctl status safe-trigger.service`
    *   **Stop Service:** `sudo systemctl stop safe-trigger.service`
    *   **Restart Service:** `sudo systemctl restart safe-trigger.service`
    *   **View Logs (if using journald):** `sudo journalctl -u safe-trigger.service -f` (Use `-f` to follow logs)

## API Usage

Endpoint: `/api/chat` (Accepts GET and POST)
Server Address: `http://localhost:3000` (or your server's address)

### Request Parameters/Body

| Field           | Type     | Required | Description                                                                 |
| --------------- | -------- | -------- | --------------------------------------------------------------------------- |
| `prompt`        | `string` | Yes      | Your question or input for the LLM.                                         |
| `system_prompt` | `string` | Yes      | System instructions for the LLM (e.g., "You are a helpful assistant.").     |
| `llm`           | `string` | No       | Specify LLM type: "gemini" or "openrouter". If omitted, uses any available. |
| `access_token`  | `string` | Optional | Required only if configured in `access_token.txt`.                          |

### Examples

**POST Request (using curl):**

```bash
curl -X POST "http://localhost:3000/api/chat" \
     -H "Content-Type: application/json" \
     -d '{
           "prompt": "What is the capital of France?",
           "system_prompt": "Respond concisely.",
           "llm": "openrouter",
           "access_token": "YOUR_SERVER_ACCESS_TOKEN"
         }'
```

**GET Request (URL):**

```
http://localhost:3000/api/chat?prompt=What%20is%20Rust%3F&system_prompt=Explain%20like%20I%27m%20five.&access_token=YOUR_SERVER_ACCESS_TOKEN
```

### Response Format

**Success (200 OK):**

```json
{
    "content": "The model's response text...",
    "token_type": "gemini" // or "openrouter" (Indicates which token type was used)
}
```

**Error (e.g., 400 Bad Request, 401 Unauthorized, 500 Internal Server Error):**

```json
{
    "error": "A message describing the error (e.g., Invalid access token, No available tokens, API error details...)"
}
```

## Current Limitations

-   Supports only Google Gemini and OpenRouter via specific client implementations.
-   Token management relies on a local SQLite database.
-   Single-instance deployment.

## Future Plans

-   Support for more LLM providers.
-   More sophisticated rate limiting and token management.
-   Request caching.
-   Response streaming.
-   Distributed deployment options.
