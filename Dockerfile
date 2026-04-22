FROM rust:alpine AS builder

WORKDIR /build

RUN apk add --no-cache musl-dev gcc

COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM alpine:latest

RUN apk add --no-cache openssl

WORKDIR /app

COPY --from=builder /build/target/release/nwc-relay /app/nwc-relay
COPY .env.example /app/.env.example

RUN mkdir -p /data

EXPOSE 7777 7778

VOLUME ["/data"]

ENTRYPOINT ["/app/nwc-relay"]
CMD ["run"]