FROM rust:bookworm AS builder

WORKDIR /opt/r2cn


RUN apt-get update && apt-get install -y \
    libssl-dev \
    ca-certificates 

# copy the source code, the context must be the root of the project
COPY . .

# build
RUN cargo build --release;


# final image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl-dev ca-certificates

WORKDIR /opt/r2cn

COPY --from=builder /opt/r2cn/target/release/mentor-link-api /usr/local/bin
COPY --from=builder /opt/r2cn/target/release/migration /usr/local/bin
COPY --from=builder /opt/r2cn/api/templates /opt/r2cn/templates

RUN chmod +x /usr/local/bin/mentor-link-api
RUN chmod +x /usr/local/bin/migration


CMD ["/usr/local/bin/mentor-link-api"]