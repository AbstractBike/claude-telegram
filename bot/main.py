import asyncio
import logging
import os
from telegram import Update
from telegram.ext import ApplicationBuilder, MessageHandler, filters, ContextTypes
from bot.config import Config
from bot.claude_session import ClaudeSession

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

WORK_DIR = os.path.expanduser("~")

HELP_TEXT = """*AbstractBike Claude Bot* — Claude CLI via Telegram

*Commands:*
/status — Show session uptime
/reset — Kill session and start fresh
/help — Show this message

*Claude Code skills (forwarded to Claude):*
/commit — Create a git commit
/review\\_pr — Review a pull request
/brainstorming — Brainstorm ideas
/feature\\_dev — Guided feature development

*Any other text* is forwarded directly to Claude.
"""


def make_handler(allowed_users: set[str], sessions: dict):
    async def handle_message(update: Update, context: ContextTypes.DEFAULT_TYPE):
        # Guard against anonymous or username-less users
        if update.effective_user is None or update.effective_user.username is None:
            await update.message.reply_text("Please set a Telegram username to use this bot.")
            return

        username = f"@{update.effective_user.username}"
        chat_id = update.message.chat_id
        text = (update.message.text or "").strip()

        # Authorization check
        if allowed_users and username not in allowed_users:
            await update.message.reply_text("Not authorized.")
            return

        # Built-in commands
        if text in ("/reset", "!reset"):
            if chat_id in sessions:
                await sessions[chat_id].stop()
                del sessions[chat_id]
            await update.message.reply_text("Session reset.")
            return

        if text in ("/status", "!status"):
            session = sessions.get(chat_id)
            if session and session.is_running():
                await update.message.reply_text(session.status())
            else:
                await update.message.reply_text("No active session.")
            return

        if text == "/help":
            await update.message.reply_text(HELP_TEXT, parse_mode="Markdown")
            return

        # Forward to Claude (including /commit, /review_pr, etc.)
        if chat_id not in sessions:
            sessions[chat_id] = ClaudeSession(chat_id=chat_id, work_dir=WORK_DIR)

        session = sessions[chat_id]
        response = await session.send(text)
        for chunk in [response[i:i+4096] for i in range(0, len(response), 4096)]:
            await update.message.reply_text(chunk)

    return handle_message


def main():
    config = Config.from_env()
    sessions: dict[int, ClaudeSession] = {}
    handler = make_handler(allowed_users=config.allowed_users, sessions=sessions)
    app = ApplicationBuilder().token(config.token).build()
    # Accept ALL text including commands (/commit, /help, etc.)
    app.add_handler(MessageHandler(filters.TEXT, handler))
    logger.info("Bot started")
    app.run_polling()


if __name__ == "__main__":
    main()
