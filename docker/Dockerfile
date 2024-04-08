FROM rust:slim

RUN apt-get update \
    && apt-get install -y \
    git pkg-config libssl-dev

COPY ./ ./

RUN cargo build --release

VOLUME ["/data"]
WORKDIR /data

CMD ["/target/release/mchprs"]