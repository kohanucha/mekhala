# Builder stage
FROM rust:1.81-alpine AS builder

RUN apk add --no-cache musl-dev perl make gcc

WORKDIR /usr/src/nwc-relay
COPY . .

RUN cargo build --release

# Runtime stage
FROM alpine:latest

RUN apk add --no-cache libgcc
RUN mkdir -p /data

COPY --from=builder /usr/src/nwc-relay/target/release/nwc-relay /usr/local/bin/nwc-relay
COPY --from=builder /usr/src/nwc-relay/target/release/nwc-relay-cli /usr/local/bin/nwc-relay-cli

EXPOSE 7777 7778

ENV DATA_DIR=/data
ENV RELAY_PORT=7777
ENV HTTP_PORT=7778

ENTRYPOINT ["nwc-relay"]