FROM rust:1-slim-buster as build

RUN USER=root cargo new --bin compprog_bot
WORKDIR /compprog_bot

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

RUN cargo build --release
RUN rm ./src/*
COPY ./src ./src

RUN rm ./target/release/deps/compprog_bot*
RUN cargo build --release

FROM debian:buster-slim

COPY --from=build /compprog_bot/target/release/deps/compprog_bot .
CMD ["./compprog_bot"]
