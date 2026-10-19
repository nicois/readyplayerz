# The game server, for Fly (see fly.toml). Only the server is built: the page itself is on Cloudflare Pages.
FROM rust:1-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p server

FROM debian:bookworm-slim
COPY --from=build /src/target/release/server /usr/local/bin/server
CMD ["server"]
