# tests/test_claude_session.py
import pytest
import asyncio
from unittest.mock import AsyncMock, MagicMock, patch
from datetime import datetime


def test_session_initial_state():
    from bot.claude_session import ClaudeSession
    s = ClaudeSession(chat_id=123, work_dir="/tmp")
    assert s.chat_id == 123
    assert s.process is None
    assert s.started_at is None


@pytest.mark.asyncio
async def test_session_start_spawns_process():
    from bot.claude_session import ClaudeSession
    s = ClaudeSession(chat_id=123, work_dir="/tmp")

    mock_proc = AsyncMock()
    mock_proc.stdin = AsyncMock()
    mock_proc.stdout = AsyncMock()
    mock_proc.returncode = None

    with patch("asyncio.create_subprocess_exec", return_value=mock_proc) as mock_exec:
        await s.start()
        mock_exec.assert_called_once()
        assert "claude" in mock_exec.call_args[0]
    assert s.process is mock_proc
    assert s.started_at is not None


@pytest.mark.asyncio
async def test_session_stop_terminates_process():
    from bot.claude_session import ClaudeSession
    s = ClaudeSession(chat_id=123, work_dir="/tmp")

    mock_proc = AsyncMock()
    mock_proc.returncode = None
    s.process = mock_proc
    s.started_at = datetime.now()

    await s.stop()
    mock_proc.terminate.assert_called_once()
    assert s.process is None


@pytest.mark.asyncio
async def test_session_is_running_false_when_no_process():
    from bot.claude_session import ClaudeSession
    s = ClaudeSession(chat_id=123, work_dir="/tmp")
    assert not s.is_running()


def test_session_status_shows_uptime():
    from bot.claude_session import ClaudeSession
    from datetime import timedelta
    s = ClaudeSession(chat_id=123, work_dir="/tmp")
    s.started_at = datetime.now() - timedelta(minutes=5)
    s.process = MagicMock()
    s.process.returncode = None
    status = s.status()
    assert "5m" in status or "running" in status.lower()
