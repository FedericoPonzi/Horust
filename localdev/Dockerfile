## This Dockerfile is used for building Horust in a conatiner while developing locally.
## It follows https://blog.mgattozzi.dev/caching-rust-docker-builds/ AND SHOULD NOT BE USED IN PRODUCTION

FROM rust:1.42 as builder
WORKDIR /usr/src/myapp
ARG CARGO_PARAMS
ARG GIT_COMMIT
ARG GIT_BRANCH
ARG IMAGE_NAME
# The dummy file is just used to get the deps into our project
# Note that COPY only runs if the files it is provided has changed, otherwise the Docker cache is hit
COPY localdev/dummy.rs Cargo.toml Cargo.lock ./
# This uses the dummy Rust program to compile all deps
RUN sed -i 's^src/main.rs^localdev/dummy.rs^' Cargo.toml
RUN echo "Running cargo build with dummy program to build all deps: $CARGO_PARAMS" && cargo build $CARGO_PARAMS
# Now switch back to the main problem before actually creating the Horust binary
RUN sed -i 's^localdev/dummy.rs^src/main.rs^' Cargo.toml
# Get everything else into the conatiner
COPY ../ .
# And now build without re-compiling all the things! Profit!
RUN echo "Running cargo build with actual Rust code to build all deps: $CARGO_PARAMS" && cargo build $CARGO_PARAMS

FROM debian:buster-slim
COPY --from=builder /usr/src/myapp/target/release/horust /sbin/horust
RUN mkdir -p /etc/horust/services/ && apt-get update && apt-get install bash
ENV HORUST_LOG info
ENV GIT_COMMIT=$GIT_COMMIT
ENV GIT_BRANCH=$GIT_BRANCH
ENV CARGO_PARAMS=$CARGO_PARAMS

ENTRYPOINT ["/sbin/horust"]
RUN echo "{\"rev\":\"$GIT_COMMIT\",\"branch\":\"${GIT_BRANCH}\",\"cargo-params\":\"${CARGO_PARAMS}\" }" > /etc/horust/buildinfo.json
CMD ["--services-path", "/etc/horust/services/"]
