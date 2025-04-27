# Use Fedora as the build environment
FROM fedora:latest

# Install Rust and build tools
RUN dnf update -y && \
    dnf install -y rust cargo gcc make openssl-devel

# Set workdir
WORKDIR /app

# Copy project files
COPY . .

# Build the release binary
RUN cargo build --release

# The resulting binary will be at /app/target/release/
