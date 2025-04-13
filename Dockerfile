FROM rust:latest as builder
WORKDIR /usr/src/app
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    perl-base \
    libfindbin-libs-perl \
    make \
    clang \
    libclang-dev
COPY . .
RUN cargo build --release

FROM debian:bookworm
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/src/app/target/release/tgv /usr/local/bin/
ENTRYPOINT ["tgv"] 
