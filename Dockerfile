# Use the official Rust image as a base
FROM rust:latest AS builder

# Set the working directory
WORKDIR /usr/src/safe-trigger

# Copy the data.db file
COPY data.db ./data.db

# Copy the Cargo.toml and Cargo.lock files
COPY Cargo.toml Cargo.lock ./

# Copy the source code
COPY src ./src

# Build the project in release mode
RUN cargo build --release

# Use a minimal image for the final stage
FROM debian:bullseye-slim

# Set the working directory
WORKDIR /usr/local/bin

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/safe-trigger/target/release/safe-trigger .

# Copy the data.db file from the builder stage
COPY --from=builder /usr/src/safe-trigger/data.db .

# Expose the port the application listens on (assuming default Axum port 3000, adjust if needed)
EXPOSE 3000

# Command to run the application
CMD ["./safe-trigger"]
