FROM rust:bookworm AS builder
COPY . /app
WORKDIR /app
RUN ["cargo", "build", "--release"]

FROM debian:bookworm-slim
RUN mkdir /app && apt-get update && apt-get install -y openssl curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app/
COPY --from=builder /app/target/release/stab ./
EXPOSE 5656 3400
ENTRYPOINT ["./stab","server"]