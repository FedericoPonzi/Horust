FROM rust:bullseye as builder

COPY . /app/
WORKDIR /app
RUN cargo build --release

FROM scratch
COPY --from=builder /app/target/release/horust .
ENTRYPOINT [ "./horust" ]
