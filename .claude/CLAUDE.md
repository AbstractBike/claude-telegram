# Claude Telegram Bot

This is the claude-telegram bot — a Telegram-to-Claude-CLI bridge.

## Language
All code and docs in English.

## Structure
- `bot/main.py` — Telegram handlers entry point
- `bot/claude_session.py` — persistent Claude CLI subprocess manager
- `bot/config.py` — configuration from env vars
- `flake.nix` — Nix flake with Home Manager module
- `tests/` — pytest tests
