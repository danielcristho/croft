"""Prometheus HTTP API client: instant and range queries, with the server's error message on failure."""

import json
import os
import urllib.error
import urllib.parse
import urllib.request

DEFAULT_URL = "http://prometheus.127.0.0.1.sslip.io:30080"


class PrometheusError(RuntimeError):
    pass


class Prometheus:
    def __init__(self, url: str | None = None, timeout_s: float = 60):
        self.url = (url or os.environ.get("CROFT_PROMETHEUS") or os.environ.get("PROM_HOST") or DEFAULT_URL).rstrip("/")
        self.timeout_s = timeout_s

    def query(self, query: str, at_s: float | None = None) -> list[dict]:
        params = {"query": query} if at_s is None else {"query": query, "time": at_s}
        return self._call("query", params)

    def query_range(self, query: str, start_s: float, end_s: float, step_s: float) -> list[dict]:
        return self._call("query_range", {"query": query, "start": start_s, "end": end_s, "step": step_s})

    def _call(self, path: str, params: dict) -> list[dict]:
        body = urllib.parse.urlencode(params).encode()
        try:
            with urllib.request.urlopen(f"{self.url}/api/v1/{path}", body, timeout=self.timeout_s) as response:
                answer = json.load(response)
        except urllib.error.HTTPError as error:
            answer = _json_or_none(error.read())
            if answer is None:
                raise PrometheusError(f"{self.url}: HTTP {error.code}") from error
        except OSError as error:
            raise PrometheusError(f"{self.url}: {error}") from error
        if answer.get("status") != "success":
            raise PrometheusError(f"{answer.get('errorType', 'error')}: {answer.get('error', answer)}")
        return answer["data"]["result"]


def _json_or_none(raw: bytes) -> dict | None:
    try:
        return json.loads(raw)
    except ValueError:
        return None
