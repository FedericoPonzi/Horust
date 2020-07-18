## This Dockerfile is used for building Horust in a conatiner while developing locally.
## It follows https://blog.mgattozzi.dev/caching-rust-docker-builds/ AND SHOULD NOT BE USED IN PRODUCTION

FROM rust:1.42
WORKDIR /usr/src/myapp
ARG CARGO_COMMAND
# The dummy file is just used to get the deps into our project
# Note that COPY only runs if the files it is provided has changed, otherwise the Docker cache is hit
COPY Cargo.toml Cargo.lock ./
COPY src/dummy.rs ./src/dummy.rs
# This uses a dummy Rust program to compile all deps
RUN sed -i 's^src/main.rs^src/dummy.rs^' Cargo.toml
RUN echo "Running cargo $CARGO_COMMAND with dummy program: " && cargo $CARGO_COMMAND
# Now switch back to the main problem before actually creating the Horust binary
RUN sed -i 's^src/dummy.rs^src/main.rs^' Cargo.toml
# Get everything else into the conatiner
COPY . .
# And now build without re-compiling all the things! Profit!
RUN echo "Running cargo $CARGO_COMMAND with actual Horust:" && cargo $CARGO_COMMAND

