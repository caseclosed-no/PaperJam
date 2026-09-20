FROM rust:1.90-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends cups-client ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 10001 paperjam
WORKDIR /app
COPY --from=builder /build/target/release/paperjam /usr/local/bin/paperjam
RUN mkdir -p /app/data/games /app/data/pdfs /app/data/state && chown -R paperjam:paperjam /app/data/state
USER paperjam
EXPOSE 8080
ENV PAPERJAM_BIND=0.0.0.0:8080
CMD ["paperjam"]
