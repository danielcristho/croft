"""Applications under test, read from apps.json: where each runs, how it is reached, its scaler calibration, its services and
its endpoint mix."""

import json
from dataclasses import dataclass, field, replace
from pathlib import Path

from .profiles import Endpoint

ROOT = Path(__file__).resolve().parent.parent
CATALOG = ROOT / "apps.json"
# support runs beside the app and serves no request: service discovery, tracing, telemetry, feature flags.
ROLES = ("gateway", "business", "cache", "database", "support")


@dataclass(frozen=True)
class Calibration:
    """What croft scale needs to turn a syscall rate into CPU demand, measured per application."""

    syscall: str
    calls_per_request: float
    cpu_per_request_ms: float


@dataclass(frozen=True)
class Service:
    role: str
    language: str


@dataclass(frozen=True)
class Mix:
    """Weighted endpoints for --mix runs, with the scale and calibration that replace the app's own when set."""

    endpoints: tuple[Endpoint, ...]
    scale: float | None = None
    scaler: Calibration | None = None
    # {user} takes one of this many users per run, so runs never share a session or a cart.
    users: int = 1


@dataclass(frozen=True)
class App:
    name: str
    namespace: str
    deployment: str
    container: str
    # A folder with k8s.yaml for the app and hpa.yaml for its baseline autoscaler.
    manifests: Path
    host: str
    path: str
    health: str
    scaler: Calibration | None
    # The --profile scale that keeps a burst between one pod saturated and ten pods' worth.
    scale: float = 1.0
    # Every Deployment of the namespace, by name.
    services: dict[str, Service] = field(default_factory=dict)
    mix: Mix | None = None

    @property
    def selector(self) -> str:
        return f"app={self.deployment}"

    @property
    def url(self) -> str:
        return f"{self.host}{self.path}"

    def users(self, mix: bool = False) -> int:
        return self.mix.users if mix and self.mix else 1

    def endpoints(self, mix: bool = False) -> list[Endpoint]:
        if not mix:
            return [Endpoint("default", self.path)]
        if self.mix is None:
            raise SystemExit(f"{self.name}: apps.json has no mix")
        return list(self.mix.endpoints)

    def with_mix(self) -> "App":
        """The app as --mix runs see it: the mix's scale and calibration in place of its own."""
        self.endpoints(mix=True)
        return replace(self, scale=self.mix.scale or self.scale, scaler=self.mix.scaler or self.scaler)


def parse_mix(entry: dict) -> Mix:
    endpoints = tuple(Endpoint.parse(endpoint) for endpoint in entry["endpoints"])
    names = [endpoint.name for endpoint in endpoints]
    if not endpoints or len(set(names)) != len(names):
        raise ValueError(f"mix: endpoints need unique names, got {names}")
    if not any(endpoint.weight for endpoint in endpoints):
        raise ValueError("mix: at least one endpoint needs a weight above 0")
    for endpoint in endpoints:
        if endpoint.before and (endpoint.before not in names or endpoints[names.index(endpoint.before)].before):
            raise ValueError(f"mix: {endpoint.name} comes after {endpoint.before!r}, which needs to be an endpoint with no before")
    users = entry.get("users", 1)
    if not isinstance(users, int) or users < 1:
        raise ValueError(f"mix: users needs to be a whole number of at least 1, got {users!r}")
    scaler = entry.get("scaler")
    return Mix(endpoints, entry.get("scale"), Calibration(**scaler) if scaler else None, users)


def load(path: Path = CATALOG) -> dict[str, App]:
    """Every app in the catalog; a missing or unknown key stops the harness before it touches the cluster."""
    apps = {}
    for name, entry in json.loads(path.read_text()).items():
        entry = dict(entry)
        scaler = entry.pop("scaler", None)
        mix = entry.pop("mix", None)
        try:
            app = App(
                name=name,
                manifests=ROOT / entry.pop("manifests"),
                scaler=Calibration(**scaler) if scaler else None,
                services={service: Service(**fields) for service, fields in entry.pop("services", {}).items()},
                mix=parse_mix(mix) if mix else None,
                **entry,
            )
        except (KeyError, TypeError, ValueError) as error:
            raise SystemExit(f"{path}: app {name}: {error}") from error
        unknown = {service: s.role for service, s in app.services.items() if s.role not in ROLES}
        if unknown:
            raise SystemExit(f"{path}: app {name}: roles {unknown} are not among {ROLES}")
        if app.services and app.deployment not in app.services:
            raise SystemExit(f"{path}: app {name}: the scaled deployment {app.deployment} is not among its services")
        for manifest in ("k8s.yaml", "hpa.yaml"):
            if not (app.manifests / manifest).is_file():
                raise SystemExit(f"{path}: app {name}: {app.manifests / manifest} is missing")
        apps[name] = app
    return apps


APPS = load()


# python3 -m experiment.apps [<app> <field>]: the app names, or one field of an app, for the justfile.
if __name__ == "__main__":
    import sys

    if len(sys.argv) == 3:
        print(getattr(APPS[sys.argv[1]], sys.argv[2]))
    else:
        for app in APPS.values():
            print(f"{app.name:18} {app.namespace:10} {app.url}")
