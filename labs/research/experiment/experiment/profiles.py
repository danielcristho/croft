"""Offered load: profiles in requests per second, with the parameters of the cloud2023 cloud-dynamics scenarios, and
the endpoints a request goes to."""

import json
import math
import random
from dataclasses import dataclass, field, replace

# Consts
METHODS = ("GET", "POST")


@dataclass(frozen=True)
class Endpoint:
    """One request of an application; a mix sends each with a share of weight over the sum of weights.

    before names an endpoint sent first in the same slot, as one timed request; a weight of 0 sends an endpoint only
    there. {n} in the path, body or headers is replaced by a number unique to each send, such as a new user, and
    {user} by one of the run's users, such as a session.
    """

    name: str
    path: str
    weight: float = 1.0
    method: str = "GET"
    body: str = ""
    content_type: str = ""
    headers: dict[str, str] = field(default_factory=dict)
    before: str = ""

    @classmethod
    def parse(cls, entry: dict) -> "Endpoint":
        """An endpoint of apps.json; an object body is sent as JSON, a string body as a form."""
        entry = dict(entry)
        body = entry.pop("body", "")
        if isinstance(body, dict):
            entry.setdefault("content_type", "application/json")
            body = json.dumps(body, separators=(",", ":"))
        elif body:
            entry.setdefault("content_type", "application/x-www-form-urlencoded")
        endpoint = cls(body=body, **entry)
        if endpoint.weight < 0 or endpoint.method not in METHODS or not endpoint.path.startswith("/"):
            raise ValueError(f"endpoint {endpoint.name}: needs a weight of 0 or more, a method of {METHODS} and a path from /")
        if endpoint.body and endpoint.method != "POST":
            raise ValueError(f"endpoint {endpoint.name}: a body needs POST")
        return endpoint


@dataclass(frozen=True)
class Profile:
    name: str
    duration_s: float
    rps: float = 0.0
    start_rps: float = 0.0
    end_rps: float = 0.0
    base_rps: float = 0.0
    burst_rps: float = 0.0
    burst_start_s: float = 0.0
    burst_duration_s: float = 0.0
    min_rps: float = 0.0
    max_rps: float = 0.0
    period_s: float = 60.0
    change_interval_s: float = 30.0
    seed: int = 42

    def scaled(self, factor: float) -> "Profile":
        rates = ["rps", "start_rps", "end_rps", "base_rps", "burst_rps", "min_rps", "max_rps"]
        return replace(self, **{name: getattr(self, name) * factor for name in rates})


PROFILES = {
    "constant": Profile("constant", 30, rps=25),
    "ramp": Profile("ramp", 600, start_rps=10, end_rps=200),
    "burst": Profile("burst", 900, base_rps=10, burst_rps=150, burst_start_s=180, burst_duration_s=120),
    "periodic": Profile("periodic", 600, min_rps=10, max_rps=100, period_s=120),
    "variable": Profile("variable", 600, min_rps=10, max_rps=100, change_interval_s=30, seed=42),
}


class Rate:
    """Target rate at a time into the run; variable draws a new level every change interval."""

    def __init__(self, profile: Profile):
        self.profile = profile
        self.rng = random.Random(profile.seed)
        self.bucket = -1
        self.level = 0.0

    def at(self, elapsed_s: float) -> float:
        p = self.profile
        if p.name == "constant":
            return max(0.0, p.rps)
        if p.name == "ramp":
            fraction = min(1.0, max(0.0, elapsed_s / max(p.duration_s, 1e-9)))
            return max(0.0, p.start_rps + (p.end_rps - p.start_rps) * fraction)
        if p.name == "burst":
            inside = p.burst_start_s <= elapsed_s < p.burst_start_s + p.burst_duration_s
            return max(0.0, p.burst_rps if inside else p.base_rps)
        if p.name == "periodic":
            middle, amplitude = (p.min_rps + p.max_rps) / 2, (p.max_rps - p.min_rps) / 2
            return max(0.0, middle + amplitude * math.sin(2 * math.pi * elapsed_s / max(p.period_s, 1e-9)))
        bucket = int(elapsed_s // max(p.change_interval_s, 1e-9))
        if bucket != self.bucket:
            self.bucket, self.level = bucket, self.rng.uniform(p.min_rps, p.max_rps)
        return max(0.0, self.level)
