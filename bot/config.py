import os
from dataclasses import dataclass, field


@dataclass
class Config:
    token: str
    allowed_users: set[str] = field(default_factory=set)

    @classmethod
    def from_env(cls) -> "Config":
        token = os.environ.get("TELEGRAM_TOKEN", "")
        if not token:
            raise ValueError("TELEGRAM_TOKEN env var is required")
        allowed_raw = os.environ.get("ALLOWED_USERS", "")
        allowed = {u.strip() for u in allowed_raw.split(",") if u.strip()}
        return cls(token=token, allowed_users=allowed)
