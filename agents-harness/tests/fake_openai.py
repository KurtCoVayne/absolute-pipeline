"""A fake OpenAI server for the tests: `POST /v1/responses` (the endpoint
openai-agents 0.24 uses by default) answered from a script of steps, every
request recorded with its headers and body."""

from __future__ import annotations

import json
import threading
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any


@dataclass
class ToolCalls:
    """The model asks for these function calls in one turn."""

    calls: list[tuple[str, dict[str, Any]]]
    input_tokens: int = 1_000
    output_tokens: int = 100
    cached_tokens: int = 0


@dataclass
class Message:
    """The model's final text (the structured output as JSON text)."""

    text: str
    input_tokens: int = 1_000
    output_tokens: int = 100
    cached_tokens: int = 0


@dataclass
class Status:
    """An HTTP error the server answers with (429, 500, …)."""

    code: int
    body: dict[str, Any] = field(default_factory=lambda: {"error": {"message": "busy", "type": "rate_limit"}})
    headers: dict[str, str] = field(default_factory=dict)


Step = ToolCalls | Message | Status


@dataclass
class Recorded:
    path: str
    headers: dict[str, str]
    body: dict[str, Any]

    @property
    def instructions(self) -> str:
        return str(self.body.get("instructions", ""))

    @property
    def input_text(self) -> str:
        items = self.body.get("input", [])
        if isinstance(items, str):
            return items
        out = []
        for it in items:
            c = it.get("content")
            if isinstance(c, str):
                out.append(c)
            elif isinstance(c, list):
                out.extend(str(p.get("text", "")) for p in c if isinstance(p, dict))
            if it.get("type") == "function_call_output":
                out.append(str(it.get("output", "")))
        return "\n".join(out)

    @property
    def tool_names(self) -> list[str]:
        return [t.get("name", "") for t in self.body.get("tools", []) if t.get("type") == "function"]


class FakeOpenAI:
    def __init__(self, script: list[Step] | None = None) -> None:
        self.script: list[Step] = list(script or [])
        self.requests: list[Recorded] = []
        self.lock = threading.Lock()
        self.counter = 0
        server = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a: Any) -> None:
                pass

            def do_GET(self) -> None:
                if self.path.endswith("/v1/models"):
                    self._json(200, {"object": "list", "data": [{"id": "gpt-6-luna", "object": "model"}]})
                else:
                    self._json(404, {"error": {"message": "no such route"}})

            def do_POST(self) -> None:
                n = int(self.headers.get("Content-Length", "0"))
                body = json.loads(self.rfile.read(n) or b"{}")
                with server.lock:
                    server.requests.append(Recorded(self.path, {k.lower(): v for k, v in self.headers.items()}, body))
                    step = server.script.pop(0) if server.script else Message("{}")
                    server.counter += 1
                    n_id = server.counter
                if not self.path.endswith("/v1/responses"):
                    self._json(404, {"error": {"message": f"the fake serves /v1/responses, not {self.path}"}})
                    return
                if isinstance(step, Status):
                    self._json(step.code, step.body, step.headers)
                    return
                self._json(200, server.render(step, n_id, body.get("model", "gpt-6-luna")))

            def _json(self, code: int, payload: dict[str, Any], headers: dict[str, str] | None = None) -> None:
                data = json.dumps(payload).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                for k, v in (headers or {}).items():
                    self.send_header(k, v)
                self.end_headers()
                self.wfile.write(data)

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self.httpd.server_port}/v1"

    def start(self) -> FakeOpenAI:
        self.thread.start()
        return self

    def stop(self) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()

    @staticmethod
    def render(step: ToolCalls | Message, n: int, model: str) -> dict[str, Any]:
        if isinstance(step, ToolCalls):
            output = [
                {"id": f"fc_{n}_{i}", "type": "function_call", "call_id": f"call_{n}_{i}", "name": name, "arguments": json.dumps(args), "status": "completed"}
                for i, (name, args) in enumerate(step.calls)
            ]
        else:
            output = [{"id": f"msg_{n}", "type": "message", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": step.text, "annotations": []}]}]
        return {
            "id": f"resp_{n}",
            "object": "response",
            "created_at": 0,
            "model": model,
            "status": "completed",
            "output": output,
            "parallel_tool_calls": True,
            "tool_choice": "auto",
            "tools": [],
            "usage": {
                "input_tokens": step.input_tokens,
                "output_tokens": step.output_tokens,
                "total_tokens": step.input_tokens + step.output_tokens,
                "input_tokens_details": {"cached_tokens": step.cached_tokens, "cache_write_tokens": 0},
                "output_tokens_details": {"reasoning_tokens": 0},
            },
        }
