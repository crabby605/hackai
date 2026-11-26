# HackAI

Local-first AI coding-time tracker. Extracts features—never prompts, code, paths, or tool output.

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL_3.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Next.js](https://img.shields.io/badge/Next.js-15-black.svg)](https://nextjs.org)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-blue.svg)](https://www.postgresql.org)

[![GitHub release](https://img.shields.io/github/v/release/crabby605/hackai?label=CLI%20release)](https://github.com/crabby605/hackai/releases/latest)
[![GitHub commits](https://img.shields.io/github/last-commit/crabby605/hackai/main?label=last%20commit)](https://github.com/crabby605/hackai/commits/main)
[![CLI downloads](https://img.shields.io/github/downloads/crabby605/hackai/total?label=CLI%20downloads)](https://github.com/crabby605/hackai/releases)

## Quick start

```bash
# Install CLI
curl -fsSL https://github.com/crabby605/hackai/releases/latest/download/install-cli.sh | bash

# Track a project
cd your-project && hackai track --dashboard

# Open local dashboard
hackai dash
```

## Architecture

| Component | Tech | Port |
|-----------|------|------|
| **CLI** | Rust (Axum, SQLx, ratatui) | — |
| **API Server** | Rust + PostgreSQL | 3000 |
| **Web Dashboard** | Next.js 15 + Tailwind v4 | 3001 |

## Documentation

- [Local build & development](BUILD.md)
- [API reference](docs/API.md)
- [CLI commands](crates/cli/README.md)

## Privacy

- Raw prompts, code, file paths, and tool output **never leave your machine**
- Only derived features (classification labels, timing, PESDO scores) sync opt-in
- AGPLv3 — self-host freely, contribute back

## License

AGPL-3.0 — see [LICENSE](LICENSE).