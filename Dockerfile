FROM rust:1-slim-buster as build

RUN USER=root cargo new --bin daily-compprog
WORKDIR /daily-compprog

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

RUN cargo build --release
RUN rm ./src/*
COPY ./src ./src

RUN rm ./target/release/deps/daily_compprog*
RUN cargo build --release

FROM debian:buster-slim

COPY --from=build /daily-compprog/target/release/daily-compprog .
CMD ["./daily-compprog"]
