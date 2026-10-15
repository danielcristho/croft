"""Offered-load profiles in requests per second, with the parameters of the cloud2023 cloud-dynamics scenarios."""

import math
import random
from dataclasses import dataclass, replace


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
    "burst": Profile("burst", 600, base_rps=10, burst_rps=150, burst_start_s=180, burst_duration_s=120),
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
