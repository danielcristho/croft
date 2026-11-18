"""Line charts of signal frames, one line per pod or other label, with the signal's unit on the y axis."""

import pandas as pd

from .metrics import wide


def lines(frame: pd.DataFrame, by: str | list[str] = "pod", ax=None, title: str | None = None):
    """Draws frame as one line per value of by and returns the axes."""
    table = wide(frame, by)
    ax = table.plot(ax=ax, linewidth=1.5, legend=table.shape[1] > 1)
    ax.set_xlabel("time (UTC)")
    ax.set_ylabel(frame.attrs.get("unit", "value"))
    ax.set_title(title or frame.attrs.get("signal", ""))
    ax.grid(alpha=0.3)
    return ax
