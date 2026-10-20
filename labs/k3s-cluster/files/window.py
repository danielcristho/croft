"""Writes the last hours of a croft dataset to stdout; rows are in time order, so the start is found by bisection."""

import re
import shutil
import sys

UNIX_MS = re.compile(rb'"unix_ms":(\d+)')
MS_PER_HOUR = 3600 * 1000


def ms_near(handle, offset):
    """Time of the first whole row after offset, or None past the end."""
    handle.seek(offset)
    if offset:
        handle.readline()
    for _ in range(50):
        line = handle.readline()
        if not line:
            return None
        match = UNIX_MS.search(line)
        if match:
            return int(match.group(1))
    return None


def main(path, hours):
    with open(path, "rb") as handle:
        size = handle.seek(0, 2)
        handle.seek(max(size - 1_000_000, 0))
        times = [int(ms) for ms in UNIX_MS.findall(handle.read())]
        if not times:
            return
        latest = max(times)
        since = latest - hours * MS_PER_HOUR
        low, high = 0, size
        while high - low > 1 << 20:
            middle = (low + high) // 2
            ms = ms_near(handle, middle)
            if ms is None or ms >= since:
                high = middle
            else:
                low = middle
        handle.seek(low)
        if low:
            handle.readline()
        for line in handle:
            match = UNIX_MS.search(line, 0, 40)
            if match and int(match.group(1)) >= since:
                sys.stdout.buffer.write(line)
                break
        shutil.copyfileobj(handle, sys.stdout.buffer, 1 << 20)


if __name__ == "__main__":
    main(sys.argv[1], float(sys.argv[2]))
