FROM rust:1.85-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p hackai-server --bin hackai-server --bin hackai-migrate

FROM node:22-bookworm-slim AS web-builder
WORKDIR /src/web
ARG NEXT_PUBLIC_API_URL=http://api:3000
ENV NEXT_PUBLIC_API_URL=$NEXT_PUBLIC_API_URL
COPY web/package.json web/package-lock.json* web/bun.lock* ./
RUN if [ -f package-lock.json ]; then npm ci; else npm install; fi
COPY web ./
RUN npm run build

FROM debian:bookworm-slim AS api
RUN useradd --system --uid 10001 --create-home hackai
COPY --from=rust-builder /src/target/release/hackai-server /usr/local/bin/hackai-server
USER hackai
EXPOSE 3000
CMD ["hackai-server"]

FROM debian:bookworm-slim AS migrate
COPY --from=rust-builder /src/target/release/hackai-migrate /usr/local/bin/hackai-migrate
CMD ["hackai-migrate"]

FROM node:22-bookworm-slim AS web
WORKDIR /app
ENV NODE_ENV=production
ENV PORT=3001
ENV HOSTNAME=0.0.0.0
COPY --from=web-builder /src/web/public ./public
COPY --from=web-builder /src/web/.next/standalone ./
COPY --from=web-builder /src/web/.next/static ./.next/static
USER node
EXPOSE 3001
CMD ["node", "server.js"]
