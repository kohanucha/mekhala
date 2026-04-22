# Builder stage
FROM rust:1.81-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev perl make gcc

WORKDIR /usr/src/nwc-relay
COPY . .

# Build for release
RUN cargo build --release

# Runtime stage
FROM alpine:latest

# Install runtime dependencies (if any)
RUN apk add --no-cache libgcc

# Create data directory
RUN mkdir -p /data

# Copy binary from builder
COPY --from=builder /usr/src/nwc-relay/target/release/nwc-relay /usr/local/bin/nwc-relay

# Expose port
EXPOSE 7777

# Set data directory environment variable
ENV DATA_DIR=/data
ENV RELAY_PORT=7777

ENTRYPOINT ["nwc-relay"]
CMD ["run"]
