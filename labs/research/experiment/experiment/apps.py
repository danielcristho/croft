"""Applications under test, read from apps.json: where each runs, how it is reached, its scaler calibration and its services."""

import json
from dataclasses import dataclass, field
from pathlib import Path

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

    @property
    def selector(self) -> str:
        return f"app={self.deployment}"

    @property
    def url(self) -> str:
        return f"{self.host}{self.path}"


def load(path: Path = CATALOG) -> dict[str, App]:
    """Every app in the catalog; a missing or unknown key stops the harness before it touches the cluster."""
    apps = {}
    for name, entry in json.loads(path.read_text()).items():
        entry = dict(entry)
        scaler = entry.pop("scaler", None)
        try:
            app = App(
                name=name,
                manifests=ROOT / entry.pop("manifests"),
                scaler=Calibration(**scaler) if scaler else None,
                services={service: Service(**fields) for service, fields in entry.pop("services", {}).items()},
                **entry,
            )
        except (KeyError, TypeError) as error:
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
