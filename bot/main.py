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


def make_handler(allowed_users: set[str], sessions: dict):
    async def handle_message(update: Update, context: ContextTypes.DEFAULT_TYPE):
        username = f"@{update.effective_user.username}"
        chat_id = update.message.chat_id
        text = update.message.text or ""

        # Authorization check
        if allowed_users and username not in allowed_users:
            await update.message.reply_text("Not authorized.")
            return

        # Commands
        if text.strip() == "!reset":
            if chat_id in sessions:
                await sessions[chat_id].stop()
                del sessions[chat_id]
            await update.message.reply_text("Session reset.")
            return

        if text.strip() == "!status":
            session = sessions.get(chat_id)
            if session and session.is_running():
                await update.message.reply_text(session.status())
            else:
                await update.message.reply_text("No active session.")
            return

        # Forward to Claude
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
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, handler))
    logger.info("Bot started")
    app.run_polling()


if __name__ == "__main__":
    main()
