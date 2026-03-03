import asyncio
import os
from datetime import datetime

CLAUDE_BIN = os.environ.get("CLAUDE_PATH", "claude")


class ClaudeSession:
    def __init__(self, chat_id: int, work_dir: str):
        self.chat_id = chat_id
        self.work_dir = work_dir
        self.process: asyncio.subprocess.Process | None = None
        self.started_at: datetime | None = None

    async def start(self) -> None:
        self.process = await asyncio.create_subprocess_exec(
            CLAUDE_BIN,
            "--dangerously-skip-permissions",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
            cwd=self.work_dir,
        )
        self.started_at = datetime.now()

    async def stop(self) -> None:
        if self.process and self.process.returncode is None:
            self.process.terminate()
            try:
                await asyncio.wait_for(self.process.wait(), timeout=5)
            except asyncio.TimeoutError:
                self.process.kill()
        self.process = None
        self.started_at = None

    def is_running(self) -> bool:
        return self.process is not None and self.process.returncode is None

    def status(self) -> str:
        if not self.is_running() or not self.started_at:
            return "Session not running"
        delta = datetime.now() - self.started_at
        total_seconds = int(delta.total_seconds())
        hours, remainder = divmod(total_seconds, 3600)
        minutes, seconds = divmod(remainder, 60)
        uptime = f"{hours}h {minutes}m {seconds}s" if hours else f"{minutes}m {seconds}s"
        return f"Running — uptime: {uptime} | work_dir: {self.work_dir}"

    async def send(self, text: str) -> str:
        proc = await asyncio.create_subprocess_exec(
            CLAUDE_BIN,
            "--dangerously-skip-permissions",
            "-p", text,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
            cwd=self.work_dir,
        )
        try:
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=120)
            return stdout.decode().strip() or "(no response)"
        except asyncio.TimeoutError:
            proc.kill()
            await proc.wait()
            return "(timeout — no response after 120s)"
