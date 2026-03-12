# Build stage
FROM rust:1.85-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# Runtime stage
FROM alpine:3.21
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/target/release/claudeway /usr/local/bin/claudeway
RUN adduser -D -u 1000 claudeway
USER claudeway
EXPOSE 3000
ENV RUST_LOG=info
ENTRYPOINT ["claudeway"]
