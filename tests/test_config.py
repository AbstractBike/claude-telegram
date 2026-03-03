# tests/test_config.py
import pytest
import os


def test_config_loads_token_from_env(monkeypatch):
    monkeypatch.setenv("TELEGRAM_TOKEN", "123:abc")
    monkeypatch.setenv("ALLOWED_USERS", "")
    import importlib
    import bot.config
    importlib.reload(bot.config)
    from bot.config import Config
    cfg = Config.from_env()
    assert cfg.token == "123:abc"


def test_config_loads_allowed_users(monkeypatch):
    monkeypatch.setenv("TELEGRAM_TOKEN", "123:abc")
    monkeypatch.setenv("ALLOWED_USERS", "@alice,@bob")
    import importlib
    import bot.config
    importlib.reload(bot.config)
    from bot.config import Config
    cfg = Config.from_env()
    assert cfg.allowed_users == {"@alice", "@bob"}


def test_config_empty_allowed_users_means_open(monkeypatch):
    monkeypatch.setenv("TELEGRAM_TOKEN", "123:abc")
    monkeypatch.setenv("ALLOWED_USERS", "")
    import importlib
    import bot.config
    importlib.reload(bot.config)
    from bot.config import Config
    cfg = Config.from_env()
    assert cfg.allowed_users == set()


def test_config_raises_without_token(monkeypatch):
    monkeypatch.delenv("TELEGRAM_TOKEN", raising=False)
    import importlib
    import bot.config
    importlib.reload(bot.config)
    from bot.config import Config
    with pytest.raises(ValueError, match="TELEGRAM_TOKEN"):
        Config.from_env()
