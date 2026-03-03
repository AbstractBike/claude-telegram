# tests/test_main_handlers.py
import pytest
from unittest.mock import AsyncMock, MagicMock


def make_update(username="alice", text="hello", chat_id=42):
    update = MagicMock()
    update.effective_user.username = username  # without @
    update.message.text = text
    update.message.chat_id = chat_id
    update.message.reply_text = AsyncMock()
    return update


def make_context():
    return MagicMock()


@pytest.mark.asyncio
async def test_unauthorized_user_rejected():
    from bot.main import make_handler
    handler = make_handler(allowed_users={"@bob"}, sessions={})
    update = make_update(username="alice", text="hello")
    await handler(update, make_context())
    update.message.reply_text.assert_called_once()
    call_text = update.message.reply_text.call_args[0][0]
    assert "authorized" in call_text.lower() or "access" in call_text.lower()


@pytest.mark.asyncio
async def test_open_access_when_no_allowed_users():
    from bot.main import make_handler
    sessions = {}
    handler = make_handler(allowed_users=set(), sessions=sessions)
    update = make_update(username="anyone", text="!status", chat_id=99)
    await handler(update, make_context())
    update.message.reply_text.assert_called_once()


@pytest.mark.asyncio
async def test_reset_command_stops_session():
    from bot.main import make_handler
    mock_session = AsyncMock()
    sessions = {42: mock_session}
    handler = make_handler(allowed_users=set(), sessions=sessions)
    update = make_update(username="alice", text="!reset", chat_id=42)
    await handler(update, make_context())
    mock_session.stop.assert_called_once()
    assert 42 not in sessions


@pytest.mark.asyncio
async def test_no_username_rejected():
    from bot.main import make_handler
    handler = make_handler(allowed_users=set(), sessions={})
    update = make_update(username="alice", text="hello")
    update.effective_user.username = None  # username=None
    await handler(update, make_context())
    update.message.reply_text.assert_called_once()
    call_text = update.message.reply_text.call_args[0][0]
    assert "username" in call_text.lower()


@pytest.mark.asyncio
async def test_no_effective_user_rejected():
    from bot.main import make_handler
    handler = make_handler(allowed_users=set(), sessions={})
    update = make_update(username="alice", text="hello")
    update.effective_user = None  # anonymous channel post
    await handler(update, make_context())
    update.message.reply_text.assert_called_once()
    call_text = update.message.reply_text.call_args[0][0]
    assert "username" in call_text.lower()


@pytest.mark.asyncio
async def test_slash_reset_stops_session():
    from bot.main import make_handler
    mock_session = AsyncMock()
    sessions = {42: mock_session}
    handler = make_handler(allowed_users=set(), sessions=sessions)
    update = make_update(username="alice", text="/reset", chat_id=42)
    await handler(update, make_context())
    mock_session.stop.assert_called_once()
    assert 42 not in sessions


@pytest.mark.asyncio
async def test_help_command_returns_help_text():
    from bot.main import make_handler
    handler = make_handler(allowed_users=set(), sessions={})
    update = make_update(username="alice", text="/help", chat_id=1)
    await handler(update, make_context())
    update.message.reply_text.assert_called_once()
    call_text = update.message.reply_text.call_args[0][0]
    assert "Claude" in call_text
