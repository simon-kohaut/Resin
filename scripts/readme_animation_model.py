"""Data model of the README animation, shared by its renderers.

An abstract sketch of Reactive Circuits: four targets start as flat formulas
over shared leaves, grow downward level by level as leaves are lifted and
dropped by update frequency (one tree per target, with a copy of a node for
every path to it), and merge copies of identical sub-circuits into a shared
DAG right after each level grows. Leaf updates travel upward as pulses.

Coordinates: x in [0, WIDTH], y in [0, 1]. Only NumPy is required.
"""

import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

import numpy as np

LOOP_S = 24.0
WIDTH = 2.4

# ── Themes ─────────────────────────────────────────────────────────────────
THEMES = {
    "dark": dict(
        background="#0d1117",
        text="#b1bac4",
        guide="#21262d",
        leaf_edge="#41506a",
        # Circuits in cool hues, update rates on a warm heat scale.
        targets=["#4ade80", "#22d3ee", "#3b82f6", "#a78bfa"],
        bands={"fast": "#ff4d6d", "medium": "#ff9f1c", "slow": "#ffe14d", "slowest": "#9aa5b1"},
        flash=("#ffffff", 0.55),  # recomputed nodes brighten
    ),
    "light": dict(
        background="#ffffff",
        text="#424a53",
        guide="#eaeef2",
        leaf_edge="#c3cbd6",
        targets=["#16a34a", "#0891b2", "#2563eb", "#7c3aed"],
        bands={"fast": "#e11d48", "medium": "#ea580c", "slow": "#ca8a04", "slowest": "#94a3b8"},
        flash=("#1a1a26", 0.35),  # recomputed nodes darken
    ),
}


def rgb(hex_color):
    hex_color = hex_color.lstrip("#")
    return np.array([int(hex_color[i:i + 2], 16) / 255 for i in (0, 2, 4)])


def to_hex(color):
    return "#" + "".join(f"{int(round(np.clip(c, 0, 1) * 255)):02x}" for c in color)


# ── Structure ──────────────────────────────────────────────────────────────
# Targets on top, then three levels of shared sub-circuits ("kinds"): shared
# diamonds plus private branches (u, h, v, w, k) that hang off a single node.
TARGET_X = [0.14, 0.38, 0.62, 0.86]
TARGET_CHILDREN = [["a", "b"], ["b", "c"], ["c", "d"], ["d", "e"]]
CHILDREN = {
    "a": ["p", "u"], "b": ["p", "q"], "c": ["q", "h", "r"], "d": ["r", "s"], "e": ["s", "v"],
    "p": ["x"], "q": ["x", "y"], "r": ["y", "k", "z"], "s": ["z"], "u": [], "h": [], "v": ["w"],
    "x": [], "y": [], "z": [], "k": [], "w": [],
}
LEVEL = {**dict.fromkeys("abcde", 1), **dict.fromkeys("pqrsuhv", 2), **dict.fromkeys("xyzkw", 3)}
SHARED_X = {  # positions once merged, as fractions of the width
    "a": 0.10, "b": 0.30, "c": 0.50, "d": 0.70, "e": 0.90,
    "u": 0.05, "p": 0.21, "q": 0.38, "h": 0.51, "r": 0.63, "s": 0.79, "v": 0.95,
    "x": 0.29, "y": 0.45, "k": 0.57, "z": 0.70, "w": 0.93,
}
LEVEL_Y = {0: 0.88, 1: 0.68, 2: 0.48, 3: 0.28}
LEVEL_R = {0: 0.027, 1: 0.026, 2: 0.023, 3: 0.021}
LEAF_Y = 0.07
SPREAD = {1: 0.10, 2: 0.055, 3: 0.04}  # children's spread before merging

# Leaves by band, attached to targets ("T<i>") or kinds at their band's level.
# Each band, and so each level, updates four times less often than the one above.
BANDS = ["fast", "medium", "slow", "slowest"]
BAND_HZ = {"fast": "32 Hz", "medium": "8 Hz", "slow": "2 Hz", "slowest": "0.5 Hz"}
PERIOD = {"fast": 1.4, "medium": 2.8, "slow": 5.0, "slowest": 8.0}  # animation seconds
LEAVES = [
    ("fast", ["T0"]), ("fast", ["T1", "T2"]), ("fast", ["T3"]),
    ("medium", ["a"]), ("medium", ["b"]), ("medium", ["c"]), ("medium", ["d", "e"]),
    ("slow", ["p"]), ("slow", ["q"]), ("slow", ["r"]), ("slow", ["s"]),
    ("slow", ["u"]), ("slow", ["h"]), ("slow", ["v"]),
    ("slowest", ["x"]), ("slowest", ["y"]), ("slowest", ["y", "z"]), ("slowest", ["z"]),
    ("slowest", ["k"]), ("slowest", ["w"]),
]

# ── Timeline ───────────────────────────────────────────────────────────────
# Each level grows (lift/drop) and then merges right away, as in DAG mode;
# afterwards everything unwinds in reverse back to the flat formulas.
GROW = {1: (2.5, 4.5), 2: (6.5, 8.5), 3: (10.5, 12.5)}
MERGE = {1: (4.5, 6.5), 2: (8.5, 10.5), 3: (12.5, 14.5)}
UNWIND_MERGE = {3: (18.5, 19.5), 2: (20.0, 21.0), 1: (21.5, 22.5)}
UNWIND = {3: (19.3, 20.1), 2: (20.8, 21.6), 1: (22.3, 23.1)}
HOP_S = 0.42  # pulse travel time per edge
FLASH_S = 0.45
TRAIL_S = 0.07  # length of a pulse's trail


def ease(t, span):
    x = np.clip((t - span[0]) / (span[1] - span[0]), 0.0, 1.0)
    return x * x * (3 - 2 * x)


def lerp(a, b, s):
    return (1 - s) * np.asarray(a, float) + s * np.asarray(b, float)


def growth(t, level):
    return 1.0 if level == 0 else ease(t, GROW[level]) - ease(t, UNWIND[level])


def merge_progress(t, level):
    return 0.0 if level == 0 else ease(t, MERGE[level]) - ease(t, UNWIND_MERGE[level])


# ── Tree copies: one per target and path ───────────────────────────────────
def build_copies():
    """Every node copy as dict(kind, level, target, parent, offset, first, first_edge)."""
    copies = []

    def visit(kind, target, parent, slot, siblings):
        index = len(copies)
        copies.append(dict(kind=kind, level=LEVEL[kind], target=target, parent=parent,
                           offset=slot - (siblings - 1) / 2))
        for k, child in enumerate(CHILDREN[kind]):
            visit(child, target, index, k, len(CHILDREN[kind]))

    for target, children in enumerate(TARGET_CHILDREN):
        root = len(copies)
        copies.append(dict(kind=f"T{target}", level=0, target=target, parent=None, offset=0.0))
        for k, child in enumerate(children):
            visit(child, target, root, k, len(children))

    seen = set()
    for copy in copies:
        copy["first"] = copy["kind"] not in seen
        seen.add(copy["kind"])
        parent = copies[copy["parent"]]["kind"] if copy["parent"] is not None else None
        copy["first_edge"] = (parent, copy["kind"]) not in seen
        seen.add((parent, copy["kind"]))
    return copies


COPIES = build_copies()


def shared_x(kind):
    return TARGET_X[int(kind[1:])] if kind.startswith("T") else SHARED_X[kind]


def path_up(index):
    path = [index]
    while COPIES[path[-1]]["parent"] is not None:
        path.append(COPIES[path[-1]]["parent"])
    return path


# Leaves ordered by where their nodes end up, so their edges stay short.
_ORDER = sorted(range(len(LEAVES)), key=lambda j: np.mean([shared_x(k) for k in LEAVES[j][1]]))
LEAF_X = np.empty(len(LEAVES))
LEAF_X[_ORDER] = np.linspace(0.05, 0.95, len(LEAVES))
LEAF_POS = [np.array([x * WIDTH, LEAF_Y]) for x in LEAF_X]


@dataclass
class Theme:
    name: str

    def __post_init__(self):
        theme = THEMES[self.name]
        self.background, self.text, self.guide = theme["background"], theme["text"], theme["guide"]
        self.leaf_edge = theme["leaf_edge"]
        self.targets = [rgb(c) for c in theme["targets"]]
        self.bands = {band: rgb(c) for band, c in theme["bands"].items()}
        self.flash_color, self.flash_mix = rgb(theme["flash"][0]), theme["flash"][1]
        # A shared node mixes the colors of the targets that use it.
        owners = {}
        for copy in COPIES:
            owners.setdefault(copy["kind"], []).append(self.targets[copy["target"]])
        self.kind_color = {kind: np.mean(colors, axis=0) for kind, colors in owners.items()}


def update_events():
    """Leaf updates `(time, leaf)` within one loop, at each band's rate."""
    rng = np.random.default_rng(7)
    events = []
    for leaf, (band, _) in enumerate(LEAVES):
        t = rng.uniform(0, PERIOD[band])
        while t < LOOP_S:
            events.append((t, leaf))
            t += PERIOD[band] * rng.uniform(0.8, 1.2)
    return events


@dataclass
class Frame:
    nodes: list         # dict(pos, r, color, alpha, flash) per copy
    node_edges: list    # (start, end, color, alpha)
    leaf_edges: list    # (start, end, alpha)
    leaf_flash: np.ndarray
    pulses: list        # (head, tail, color)
    level_growth: dict  # level -> growth in [0, 1]
    node_count: float
    edge_count: float


def frame(t, events, theme):
    """Everything to draw at time `t`."""
    nodes = []
    for copy in COPIES:  # parents precede their children
        level, g = copy["level"], growth(t, copy["level"])
        m = merge_progress(t, level)
        if copy["parent"] is None:
            nodes.append(dict(pos=np.array([TARGET_X[copy["target"]] * WIDTH, LEVEL_Y[0]]),
                              r=lerp(0.048, LEVEL_R[0], growth(t, 1)),
                              color=theme.targets[copy["target"]], alpha=1.0))
            continue
        parent = nodes[copy["parent"]]
        y = LEVEL_Y[level] - 0.03 * m
        below = parent["pos"] + [copy["offset"] * SPREAD[level] * WIDTH, y - parent["pos"][1]]
        shared = np.array([shared_x(copy["kind"]) * WIDTH, y])
        nodes.append(dict(
            pos=lerp(parent["pos"], lerp(below, shared, m), g),
            r=LEVEL_R[level] * g,
            color=lerp(parent["color"], theme.kind_color[copy["kind"]], m),
            alpha=g * (1.0 if copy["first"] else 1.0 - m),
        ))

    flash = np.zeros(len(COPIES))
    leaf_flash = np.zeros(len(LEAVES))
    pulses = []
    for start, leaf in events:
        for offset in (0.0, LOOP_S):  # pulses wrapping around the loop
            age = t - start + offset
            if not 0 <= age < 5 * HOP_S + FLASH_S:
                continue
            if age < FLASH_S:
                leaf_flash[leaf] = max(leaf_flash[leaf], 1 - age / FLASH_S)
            band, kinds = LEAVES[leaf]
            for index, copy in enumerate(COPIES):
                if copy["kind"] not in kinds:
                    continue
                hops, keys = [LEAF_POS[leaf]], [None]
                for node in path_up(index):  # skip collapsed nodes
                    if np.linalg.norm(nodes[node]["pos"] - hops[-1]) > 1e-3:
                        hops.append(nodes[node]["pos"])
                        keys.append(node)
                for h in range(1, len(hops)):
                    since = age - h * HOP_S
                    if 0 <= since < FLASH_S:
                        flash[keys[h]] = max(flash[keys[h]], 1 - since / FLASH_S)

                def at(a):
                    travel = np.clip(a / HOP_S, 0, len(hops) - 1 - 1e-9)
                    segment = int(travel)
                    return lerp(hops[segment], hops[segment + 1], travel - segment)

                if age / HOP_S < len(hops) - 1:
                    pulses.append((at(age), at(max(age - TRAIL_S, 0.0)), theme.bands[band]))
    for node, value in zip(nodes, flash):
        node["flash"] = value

    node_edges = []
    for index, copy in enumerate(COPIES):
        if copy["parent"] is not None:
            m = merge_progress(t, copy["level"])
            alpha = growth(t, copy["level"]) * (1.0 if copy["first_edge"] else 1.0 - m)
            node_edges.append((nodes[copy["parent"]]["pos"], nodes[index]["pos"],
                               nodes[index]["color"], alpha))

    leaf_edges = []
    for leaf, (_, kinds) in enumerate(LEAVES):
        for index, copy in enumerate(COPIES):
            if copy["kind"] in kinds:
                g, m = growth(t, copy["level"]), merge_progress(t, copy["level"])
                weight = g * (1.0 if copy["first"] or not copy["level"] else 1.0 - m)
                leaf_edges.append((LEAF_POS[leaf], nodes[index]["pos"], max(weight, 0.15)))

    return Frame(
        nodes=nodes, node_edges=node_edges, leaf_edges=leaf_edges, leaf_flash=leaf_flash,
        pulses=pulses, level_growth={level: growth(t, level) for level in LEVEL_Y},
        node_count=sum(n["alpha"] for n in nodes),
        edge_count=sum(alpha for *_, alpha in node_edges),
    )


def video_to_gif(video, out, fps, width, colors=64):
    """Converts `video` into a looping GIF with an optimized palette."""
    with tempfile.TemporaryDirectory() as tmp:
        palette = Path(tmp) / "palette.png"
        scale = f"fps={fps},scale={width}:-1:flags=lanczos"
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(video),
                        "-vf", f"{scale},palettegen=max_colors={colors}:stats_mode=diff",
                        str(palette)], check=True)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(video), "-i", str(palette),
                        "-lavfi",
                        f"{scale}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle",
                        "-loop", "0", str(out)], check=True)
