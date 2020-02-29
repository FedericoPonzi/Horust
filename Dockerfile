FROM rust:1.41 as builder
WORKDIR /usr/src/myapp
COPY . .
ARG CARGO_PARAMS

RUN echo "Running cargo build with params: $CARGO_PARAMS" && cargo build --release $CARGO_PARAMS

FROM debian:buster-slim
COPY --from=builder /usr/src/myapp/target/release/horust /usr/local/bin/horust
RUN mkdir -p /etc/horust/services/ && apt-get update && apt install bash
ENV HORUST_LOG info
ENTRYPOINT ["/usr/local/bin/horust"]
CMD ["--services-path", "/etc/horust/services/"]