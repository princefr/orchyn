FROM rust:1.85 AS builder

WORKDIR /usr/src/app
COPY . .

# Build the server binary specifically
# We use --release for production readiness
RUN cargo build --release --bin orchyn-server
RUN cargo build --release --bin orchyn-worker

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/src/app/target/release/orchyn-server /usr/local/bin/
COPY --from=builder /usr/src/app/target/release/orchyn-worker /usr/local/bin/

ENV RUST_LOG=info
ENV PORT=3000

EXPOSE 3000

CMD ["orchyn-server"]
