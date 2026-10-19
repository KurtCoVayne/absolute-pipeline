"""The gateway's JSON tool API (contracts section 5): `POST <ABT_GATEWAY>/v1/<route>`
with `Authorization: Biscuit <token>`, retried on 429 and 5xx with backoff."""

from __future__ import annotations

import os
import random
import time
from collections.abc import Callable
from typing import Any

import httpx

GATEWAY_DEFAULT = "http://10.200.0.1:8080/gateway"
RETRYABLE = {429, 500, 502, 503, 504}


class GatewayError(Exception):
    """An answer that is not `ok`: `code` is the gateway's (`holdout`, `lineage`,
    `budget`, `preset`, `grid`, `plan_runs`, `reveal`, `forbidden`, …)."""

    def __init__(self, code: str, message: str, status: int = 0, detail: Any = None) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.status = status
        self.detail = detail

    def for_model(self) -> dict[str, Any]:
        return {"ok": False, "error": {"code": self.code, "message": self.message, "detail": self.detail}}


class Gateway:
    def __init__(
        self,
        base_url: str | None = None,
        token: str = "",
        *,
        timeout: float = 600.0,
        deadline: Callable[[], float] | None = None,
        sleep: Callable[[float], None] = time.sleep,
        max_attempts: int = 8,
    ) -> None:
        self.base_url = (base_url or os.environ.get("ABT_GATEWAY", GATEWAY_DEFAULT)).rstrip("/")
        self.token = token
        self._client = httpx.Client(timeout=timeout)
        # Seconds the retries may still spend: the job's remaining wall time by default.
        self._deadline = deadline or (lambda: 3600.0)
        self._sleep = sleep
        self._max_attempts = max_attempts
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def close(self) -> None:
        self._client.close()

    def call(self, route: str, body: dict[str, Any]) -> Any:
        """The `result` of an `ok` answer, or `GatewayError`."""
        self.calls.append((route, body))
        url = f"{self.base_url}/v1/{route}"
        headers = {"Authorization": f"Biscuit {self.token}", "Content-Type": "application/json"}
        delay = 0.5
        for attempt in range(1, self._max_attempts + 1):
            try:
                resp = self._client.post(url, json=body, headers=headers)
            except httpx.HTTPError as e:
                if attempt == self._max_attempts or self._deadline() < delay:
                    raise GatewayError("unreachable", f"{route}: {e}") from e
                self._sleep(delay)
                delay = min(delay * 2, 30.0)
                continue
            if resp.status_code in RETRYABLE and attempt < self._max_attempts:
                wait = _retry_after(resp) or delay * (1 + random.random() * 0.25)
                if self._deadline() < wait:
                    break
                self._sleep(wait)
                delay = min(delay * 2, 30.0)
                continue
            return self._answer(route, resp)
        raise GatewayError("busy", f"{route}: the gateway stayed busy ({resp.status_code})", resp.status_code)

    @staticmethod
    def _answer(route: str, resp: httpx.Response) -> Any:
        try:
            data = resp.json()
        except ValueError as e:
            raise GatewayError("malformed", f"{route}: the answer is not JSON (HTTP {resp.status_code})", resp.status_code) from e
        if isinstance(data, dict) and data.get("ok") is True:
            return data.get("result")
        err = data.get("error", {}) if isinstance(data, dict) else {}
        code = str(err.get("code") or {403: "forbidden", 400: "malformed", 404: "no_route"}.get(resp.status_code, f"http_{resp.status_code}"))
        raise GatewayError(code, str(err.get("message", resp.text[:200])), resp.status_code, err.get("detail"))


def _retry_after(resp: httpx.Response) -> float | None:
    v = resp.headers.get("Retry-After")
    if not v:
        return None
    try:
        return min(float(v), 60.0)
    except ValueError:
        return None
