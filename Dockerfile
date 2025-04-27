# Use the official Rust image as a base
FROM rust:latest AS builder

# Set the working directory
WORKDIR /usr/src/safe-trigger

# Copy the Cargo.toml and Cargo.lock files
COPY Cargo.toml Cargo.lock ./

# Copy the source code
COPY src ./src

# Add the musl target for Alpine compatibility
RUN rustup target add x86_64-unknown-linux-musl

# Build the project in release mode for the musl target
RUN cargo build --target x86_64-unknown-linux-musl --release

# Use Alpine Linux for the final stage
FROM alpine:latest

# Set the working directory
WORKDIR /usr/local/bin

# Install runtime dependencies for Alpine (OpenSSL 3 and certificates)
RUN apk add --no-cache openssl3 ca-certificates

# Copy the compiled musl binary from the builder stage
COPY --from=builder /usr/src/safe-trigger/target/x86_64-unknown-linux-musl/release/safe-trigger .

# Expose the port the application listens on (assuming default Axum port 3000, adjust if needed)
EXPOSE 3000

# Command to run the application
CMD ["./safe-trigger"]
