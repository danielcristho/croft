"""Croft datasets collected per node in CROFT_DATASET: a directory of zstd segments and a live file per node, or an older <node>.jsonl."""

import contextlib
import hashlib
import io
import itertools
import json
import os
import re
from pathlib import Path

import pandas as pd
import zstandard

from . import kube, metrics

DEFAULT_LOCATION = "/work/data/nodes"
SEGMENT = re.compile(r"croft-dataset-\d{8}T\d{6}Z\.jsonl\.zst")
LIVE = "croft-dataset.jsonl"


def files(location=None):
    """Every dataset file, each node's in time order: segments by their start stamp, then the live file."""
    root = Path(location or os.environ.get("CROFT_DATASET", DEFAULT_LOCATION))
    if not root.is_dir():
        return [root]
    found = sorted(root.glob("*.jsonl"))
    for node_dir in sorted(path for path in root.iterdir() if path.is_dir()):
        found += sorted(path for path in node_dir.iterdir() if SEGMENT.fullmatch(path.name))
        found += [node_dir / LIVE] if (node_dir / LIVE).exists() else []
    return found


def node(path):
    """The node a file belongs to: its directory for segments and live files, its name for <node>.jsonl."""
    if path.name == LIVE or SEGMENT.fullmatch(path.name):
        return path.parent.name
    return path.name.removesuffix(".jsonl")


def _open(path):
    handle = path.open("rb")
    if path.name.endswith(".zst"):
        return io.BufferedReader(zstandard.ZstdDecompressor().stream_reader(handle, closefd=True))
    return handle


def fingerprint(paths):
    digest = hashlib.sha256()
    for path in paths:
        stat = path.stat()
        digest.update(f"{path.name}:{stat.st_size}:{stat.st_mtime_ns}".encode())
    return digest.hexdigest()[:12]


@contextlib.contextmanager
def open_all(paths=None):
    handles = [_open(path) for path in (paths or files())]
    try:
        yield itertools.chain.from_iterable(handles)
    finally:
        for handle in handles:
            handle.close()


UNIX_MS = re.compile(rb'"unix_ms":(\d+)')


def _ms_near(handle, offset):
    handle.seek(offset)
    handle.readline()
    for _ in range(50):
        match = UNIX_MS.search(handle.readline())
        if match:
            return int(match.group(1))
    return None


# offset_since bisects a file, whose rows are in time order, for a byte offset at or before the first row at since_ms.
def offset_since(path, since_ms):
    low, high = 0, path.stat().st_size
    with path.open("rb") as handle:
        while high - low > 1 << 20:
            middle = (low + high) // 2
            ms = _ms_near(handle, middle)
            if ms is None or ms >= since_ms:
                high = middle
            else:
                low = middle
    return low


def read(keep=lambda line: True, paths=None, since_ms=None, until_ms=None):
    """Parses the rows whose raw bytes pass keep; returns them, each with its node, and the unreadable line count.

    since_ms and until_ms limit the read to a time window: a plain file skips to it by bisection, a zstd segment
    is decompressed from its start.
    """
    rows, unreadable = [], 0
    for path in paths or files():
        compressed = path.name.endswith(".zst")
        with _open(path) as handle:
            if since_ms is not None and not compressed:
                offset = offset_since(path, since_ms)
                handle.seek(offset)
                if offset:
                    handle.readline()
            for line in handle:
                if until_ms is not None or (since_ms is not None and compressed):
                    match = UNIX_MS.search(line, 0, 40)
                    if match and until_ms is not None and int(match.group(1)) > until_ms:
                        break
                    if match and since_ms is not None and int(match.group(1)) < since_ms:
                        continue
                if not line.strip() or not keep(line):
                    continue
                try:
                    row = json.loads(line)
                except ValueError:
                    unreadable += 1
                    continue
                row["node"] = node(path)
                rows.append(row)
    return rows, unreadable


# Where each signal sits in a row, and whether a container's processes add up or share one cgroup value.
FIELDS = {
    "syscall_rate": ("syscalls", None, True),
    "runqueue_wait": ("usage", "runqueue_s", True),
    "cpu_cores": ("cgroup_usage", "cpu_cores", False),
    "cpu_throttled_fraction": ("cgroup_usage", "cpu_throttled_fraction", False),
    "cpu_pressure": ("cgroup_usage", "cpu_pressure_avg10", False),
}
# Rows of one poll are written within a few milliseconds; polls are 2 s apart.
POLL_GAP_MS = 500
CONTAINER_ID = re.compile(rb'"container_id":"([0-9a-f]+)"')
SEGMENT_STAMP = re.compile(r"croft-dataset-(\d{8}T\d{6}Z)\.jsonl\.zst")


def signal(
    name: str,
    namespace: str | None = None,
    deployment: str | None = None,
    *,
    since: metrics.Time = "1h",
    until: metrics.Time | None = None,
    step_s: float | None = None,
    match: dict[str, str] | None = None,
    names: kube.Names | None = None,
    paths: list[Path] | None = None,
) -> pd.DataFrame:
    """Samples of one signal from the dataset files, in the shape of metrics.signal with node and container_id added.

    A summed signal is the container's total per poll, averaged over the polls of each step; a poll without a row
    for the container counts as 0, since idle processes are written every 15 polls. A cgroup signal is averaged over
    the polls that hold it. names maps container_id to (namespace, pod, container), as kube.names() returns; namespace
    and deployment need it.
    """
    if name not in FIELDS:
        raise ValueError(f"the dataset holds {', '.join(FIELDS)}; {name} is on /metrics and Prometheus only")
    spec = metrics.SIGNALS[name]
    unknown = set(match or {}) - set(spec.labels)
    if unknown:
        raise ValueError(f"{name} has no label {', '.join(sorted(unknown))}; it has {spec.labels or 'none'}")
    if (namespace or deployment) and names is None:
        raise ValueError("namespace and deployment need names, such as names=kube.names()")
    start_s, end_s, step_s = metrics.window(since, until, step_s)
    wanted = _containers(names, namespace, deployment)

    rows, stamps = [], []
    for path in between(paths or files(), start_s * 1000, end_s * 1000):
        seen: list[int] = []
        found, _ = read(_keeper(wanted, seen), [path], since_ms=int(start_s * 1000), until_ms=int(end_s * 1000))
        rows += found
        stamps += [(node(path), stamp) for stamp in seen]
    polls = _polls(pd.DataFrame(stamps, columns=["node", "unix_ms"]), step_s)
    frame = _binned(_samples(rows, name, match or {}), polls, spec.labels, FIELDS[name][2])
    frame = _named(frame, names, spec.labels)
    frame.attrs.update(signal=name, unit=spec.unit, source="dataset")
    return frame


def between(paths: list[Path], start_ms: float, end_ms: float) -> list[Path]:
    """The files that can hold rows between start_ms and end_ms: a segment spans from its stamp to the next file's."""
    chosen = []
    for index, path in enumerate(paths):
        begins = _stamp_ms(path)
        following = paths[index + 1] if index + 1 < len(paths) else None
        ends = _stamp_ms(following) if following is not None and node(following) == node(path) else None
        if begins is not None and begins > end_ms:
            continue
        if begins is not None and ends is not None and ends < start_ms:
            continue
        chosen.append(path)
    return chosen


def _stamp_ms(path: Path) -> float | None:
    found = SEGMENT_STAMP.fullmatch(path.name)
    return pd.Timestamp(found.group(1)).timestamp() * 1000 if found else None


# _keeper notes every row's time in seen, so polls in which the wanted containers wrote nothing still count.
def _keeper(wanted: set[str] | None, seen: list[int]):
    def keep(line: bytes) -> bool:
        stamp = UNIX_MS.search(line, 0, 40)
        if stamp:
            seen.append(int(stamp.group(1)))
        found = CONTAINER_ID.search(line)
        return found is not None and (wanted is None or found.group(1).decode() in wanted)

    return keep


def _containers(names: kube.Names | None, namespace: str | None, deployment: str | None) -> set[str] | None:
    if names is None or not (namespace or deployment):
        return None
    return {
        identity
        for identity, (ns, pod, _) in names.items()
        if (not namespace or ns == namespace) and (not deployment or metrics.deployment_of(pod) == deployment)
    }


def _samples(rows: list[dict], name: str, match: dict[str, str]) -> pd.DataFrame:
    section, key, _ = FIELDS[name]
    samples = []
    for row in rows:
        base = (row["unix_ms"], row["node"], row["process"]["container_id"])
        if key is None:
            samples += [(*base, s["name"], s["rate_per_sec"]) for s in row.get(section) or [] if _matches(s, match)]
        else:
            value = (row.get(section) or {}).get(key)
            if value is not None:
                samples.append((*base, value))
    labels = ["syscall"] if key is None else []
    return pd.DataFrame(samples, columns=["unix_ms", "node", "container_id", *labels, "value"])


def _matches(syscall: dict, match: dict[str, str]) -> bool:
    return "syscall" not in match or syscall["name"] == match["syscall"]


def _polls(stamps: pd.DataFrame, step_s: float) -> pd.DataFrame:
    """One row per poll and node: its first row's time and the step it falls in."""
    stamps = stamps.sort_values(["node", "unix_ms"], ignore_index=True)
    stamps["poll"] = stamps.groupby("node")["unix_ms"].diff().fillna(POLL_GAP_MS + 1).gt(POLL_GAP_MS).cumsum()
    polls = stamps.groupby(["node", "poll"])["unix_ms"].min().rename("start_ms").reset_index()
    polls["bin"] = (polls["start_ms"] / 1000 // step_s) * step_s
    return polls


def _binned(samples: pd.DataFrame, polls: pd.DataFrame, labels: tuple[str, ...], summed: bool) -> pd.DataFrame:
    keys = ["node", "container_id", *labels]
    if samples.empty:
        return pd.DataFrame(columns=["time", *keys, "value"]).astype({"time": "datetime64[ns, UTC]", "value": float})
    samples = pd.merge_asof(
        samples.sort_values("unix_ms"), polls.sort_values("start_ms"), left_on="unix_ms", right_on="start_ms", by="node"
    )
    per_poll = samples.groupby([*keys, "poll", "bin"])["value"].agg("sum" if summed else "max").reset_index()
    if summed:
        counts = polls.groupby(["node", "bin"]).size().rename("polls").reset_index()
        frame = per_poll.groupby([*keys, "bin"])["value"].sum().reset_index().merge(counts, on=["node", "bin"])
        frame["value"] = frame["value"] / frame["polls"]
    else:
        frame = per_poll.groupby([*keys, "bin"])["value"].mean().reset_index()
    frame["time"] = pd.to_datetime(frame["bin"], unit="s", utc=True)
    return frame[["time", *keys, "value"]].sort_values([*keys, "time"], ignore_index=True)


def _named(frame: pd.DataFrame, names: kube.Names | None, labels: tuple[str, ...]) -> pd.DataFrame:
    if names is None:
        return frame
    found = frame["container_id"].map(names)
    frame["namespace"] = found.map(lambda n: n[0] if isinstance(n, tuple) else None)
    frame["pod"] = found.map(lambda n: n[1] if isinstance(n, tuple) else None)
    frame["container"] = found.map(lambda n: n[2] if isinstance(n, tuple) else None)
    frame["deployment"] = frame["pod"].map(metrics.deployment_of)
    columns = ["time", "node", "namespace", "deployment", "pod", "container", "container_id", *labels, "value"]
    return frame[columns]


def syscall_rate(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("syscall_rate", namespace, deployment, **options)


def runqueue_wait(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("runqueue_wait", namespace, deployment, **options)


def cpu_cores(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("cpu_cores", namespace, deployment, **options)


def cpu_throttled_fraction(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("cpu_throttled_fraction", namespace, deployment, **options)


def cpu_pressure(namespace: str | None = None, deployment: str | None = None, **options) -> pd.DataFrame:
    return signal("cpu_pressure", namespace, deployment, **options)
