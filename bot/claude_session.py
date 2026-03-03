import asyncio
import os
from datetime import datetime


class ClaudeSession:
    def __init__(self, chat_id: int, work_dir: str):
        self.chat_id = chat_id
        self.work_dir = work_dir
        self.process: asyncio.subprocess.Process | None = None
        self.started_at: datetime | None = None

    async def start(self) -> None:
        self.process = await asyncio.create_subprocess_exec(
            "claude",
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
        if not self.is_running():
            await self.start()
        self.process.stdin.write((text + "\n").encode())
        await self.process.stdin.drain()
        response_lines = []
        try:
            while True:
                line = await asyncio.wait_for(
                    self.process.stdout.readline(), timeout=60
                )
                if not line:
                    break
                decoded = line.decode()
                response_lines.append(decoded)
                if decoded.strip() == "" and response_lines:
                    break
        except asyncio.TimeoutError:
            pass
        return "".join(response_lines).strip() or "(no response)"
