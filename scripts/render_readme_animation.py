"""Renders docs/reactive_circuits_{dark,light}.gif, the looping animation at the top of the README.

An abstract sketch of Reactive Circuits: four targets start as flat formulas
over shared leaves, grow downward level by level as leaves are lifted and
dropped by update frequency (one tree per target, with a copy of a node for
every path to it), and then merge all copies of identical sub-circuits into a
shared DAG. Leaf updates travel upward as pulses; once adapted, fast updates
only touch the top.

Run with: python scripts/render_readme_animation.py  (requires ffmpeg)
"""

import subprocess
import tempfile
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.animation import FFMpegWriter
from matplotlib.colors import to_rgb

OUT_DIR = Path(__file__).resolve().parent.parent / "docs"

FPS = 10
LOOP_S = 24.0
WIDTH = 2.4  # x extent; y spans [0, 1]

# One variant per GitHub theme; the README picks one via prefers-color-scheme.
THEMES = {
    "dark": dict(
        background="#0d1117",
        leaf_edge="#41506a",
        targets=["#2dd4bf", "#60a5fa", "#a78bfa", "#fbbf24"],
        bands={"fast": "#f472b6", "medium": "#38bdf8", "slow": "#c4b5fd", "slowest": "#94a3b8"},
        flash=([1, 1, 1], 0.55),  # recomputed nodes brighten
    ),
    "light": dict(
        background="#ffffff",
        leaf_edge="#c3cbd6",
        targets=["#0d9488", "#2563eb", "#7c3aed", "#d97706"],
        bands={"fast": "#db2777", "medium": "#0284c7", "slow": "#8b5cf6", "slowest": "#64748b"},
        flash=([0.1, 0.1, 0.15], 0.35),  # recomputed nodes darken
    ),
}

# ── Structure ──────────────────────────────────────────────────────────────
# Targets on top, then three levels of shared sub-circuits ("kinds").
TARGET_X = [0.14, 0.38, 0.62, 0.86]
TARGET_CHILDREN = [["a", "b"], ["b", "c"], ["c", "d"], ["d", "e"]]
# Shared diamonds plus private branches (u, h, v, w, k) that hang off a
# single node and are fed by a single leaf.
CHILDREN = {
    "a": ["p", "u"], "b": ["p", "q"], "c": ["q", "h", "r"], "d": ["r", "s"], "e": ["s", "v"],
    "p": ["x"], "q": ["x", "y"], "r": ["y", "k", "z"], "s": ["z"], "u": [], "h": [], "v": ["w"],
    "x": [], "y": [], "z": [], "k": [], "w": [],
}
LEVEL = {**dict.fromkeys("abcde", 1), **dict.fromkeys("pqrsuhv", 2), **dict.fromkeys("xyzkw", 3)}
# Positions once merged, as fractions of the width.
SHARED_X = {
    "a": 0.10, "b": 0.30, "c": 0.50, "d": 0.70, "e": 0.90,
    "u": 0.05, "p": 0.21, "q": 0.38, "h": 0.51, "r": 0.63, "s": 0.79, "v": 0.95,
    "x": 0.29, "y": 0.45, "k": 0.57, "z": 0.70, "w": 0.93,
}
LEVEL_Y = {0: 0.88, 1: 0.68, 2: 0.48, 3: 0.28}
LEVEL_R = {0: 0.027, 1: 0.026, 2: 0.023, 3: 0.021}
LEAF_Y = 0.07

# Leaves by band, attached to targets ("T<i>") or kinds at their band's level.
PERIOD = {"fast": 1.4, "medium": 2.8, "slow": 5.0, "slowest": 8.0}  # seconds
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
# Horizontal spread of a node's children before they merge, per level.
SPREAD = {1: 0.10, 2: 0.055, 3: 0.04}

HOP_S = 0.42  # pulse travel time per edge
FLASH_S = 0.45


def ease(t, span):
    x = np.clip((t - span[0]) / (span[1] - span[0]), 0.0, 1.0)
    return x * x * (3 - 2 * x)


def lerp(a, b, s):
    return (1 - s) * np.asarray(a, float) + s * np.asarray(b, float)


def growth(t, level):
    return 1.0 if level == 0 else ease(t, GROW[level]) - ease(t, UNWIND[level])


def merge_progress(t, level):
    if level == 0:
        return 0.0
    return ease(t, MERGE[level]) - ease(t, UNWIND_MERGE[level])


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

    roots = []
    for target, children in enumerate(TARGET_CHILDREN):
        roots.append(len(copies))
        copies.append(dict(kind=f"T{target}", level=0, target=target, parent=None))
        for k, child in enumerate(children):
            visit(child, target, roots[-1], k, len(children))

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


def apply_theme(name):
    """Sets the color globals for theme `name`."""
    global BACKGROUND, LEAF_EDGE, TARGET_COLORS, BAND_COLORS, FLASH, KIND_COLOR
    theme = THEMES[name]
    BACKGROUND, LEAF_EDGE = theme["background"], theme["leaf_edge"]
    TARGET_COLORS, BAND_COLORS, FLASH = theme["targets"], theme["bands"], theme["flash"]
    # A shared node mixes the colors of the targets that use it.
    owners = {}
    for copy in COPIES:
        owners.setdefault(copy["kind"], []).append(to_rgb(TARGET_COLORS[copy["target"]]))
    KIND_COLOR = {kind: np.mean(colors, axis=0) for kind, colors in owners.items()}


apply_theme("dark")

# Leaves ordered by where their nodes end up, so their edges stay short.
LEAF_ORDER = sorted(range(len(LEAVES)), key=lambda j: np.mean([shared_x(k) for k in LEAVES[j][1]]))
LEAF_X = np.empty(len(LEAVES))
LEAF_X[LEAF_ORDER] = np.linspace(0.05, 0.95, len(LEAVES))


def layout(t):
    """Position, radius, color and opacity of every copy at time `t`.

    A copy grows out of its parent's current position into a spot below it,
    then slides to its kind's shared position while merging. Copies under an
    already merged parent therefore coincide from the start.
    """
    state = []
    for copy in COPIES:  # parents precede their children
        level, g = copy["level"], growth(t, copy["level"])
        m = merge_progress(t, level)
        own = np.array(to_rgb(TARGET_COLORS[copy["target"]]))
        if copy["parent"] is None:
            state.append(dict(pos=np.array([TARGET_X[copy["target"]] * WIDTH, LEVEL_Y[0]]),
                              r=lerp(0.048, LEVEL_R[0], growth(t, 1)), color=own, alpha=1.0))
            continue
        parent = state[copy["parent"]]
        y = LEVEL_Y[level] - 0.03 * m
        below = parent["pos"] + [copy["offset"] * SPREAD[level] * WIDTH, y - parent["pos"][1]]
        shared = np.array([shared_x(copy["kind"]) * WIDTH, y])
        state.append(dict(
            pos=lerp(parent["pos"], lerp(below, shared, m), g),
            r=LEVEL_R[level] * g,
            color=lerp(parent["color"], KIND_COLOR[copy["kind"]], m),
            alpha=g * (1.0 if copy["first"] else 1.0 - m),
        ))
    return state


def path_up(index):
    path = [index]
    while COPIES[path[-1]]["parent"] is not None:
        path.append(COPIES[path[-1]]["parent"])
    return path


def update_events():
    rng = np.random.default_rng(7)
    events = []
    for leaf, (band, _) in enumerate(LEAVES):
        period = PERIOD[band]
        t = rng.uniform(0, period)
        while t < LOOP_S:
            events.append((t, leaf))
            t += period * rng.uniform(0.8, 1.2)
    return events


def draw_frame(ax, t, events):
    ax.clear()
    ax.set_xlim(0, WIDTH)
    ax.set_ylim(0, 1)
    ax.set_aspect("equal")
    ax.axis("off")

    state = layout(t)
    flash = np.zeros(len(COPIES))
    leaf_flash = np.zeros(len(LEAVES))
    dots = []

    # Pulses: from the leaf up through every copy it feeds, to the targets.
    for start, leaf in events:
        for offset in (0.0, LOOP_S):
            age = t - start + offset
            if not 0 <= age < 5 * HOP_S + FLASH_S:
                continue
            if age < FLASH_S:
                leaf_flash[leaf] = max(leaf_flash[leaf], 1 - age / FLASH_S)
            band, kinds = LEAVES[leaf]
            for index, copy in enumerate(COPIES):
                if copy["kind"] not in kinds:
                    continue
                hops = [np.array([LEAF_X[leaf] * WIDTH, LEAF_Y])]
                keys = [None]
                for node in path_up(index):
                    if np.linalg.norm(state[node]["pos"] - hops[-1]) > 1e-3:
                        hops.append(state[node]["pos"])
                        keys.append(node)
                    else:
                        keys[-1] = node if keys[-1] is not None else keys[-1]
                travel = age / HOP_S
                for h in range(1, len(hops)):
                    since = (travel - h) * HOP_S
                    if 0 <= since < FLASH_S:
                        flash[keys[h]] = max(flash[keys[h]], 1 - since / FLASH_S)
                segment = int(travel)
                if segment < len(hops) - 1:
                    dots.append((lerp(hops[segment], hops[segment + 1], travel - segment),
                                 BAND_COLORS[band]))

    # Leaf edges to every copy of the nodes they feed; once merged, one each.
    for leaf, (_, kinds) in enumerate(LEAVES):
        for index, copy in enumerate(COPIES):
            if copy["kind"] in kinds:
                node = state[index]
                g, m = growth(t, copy["level"]), merge_progress(t, copy["level"])
                weight = g * (1.0 if copy["first"] or not copy["level"] else 1.0 - m)
                ax.plot([LEAF_X[leaf] * WIDTH, node["pos"][0]], [LEAF_Y, node["pos"][1]],
                        color=LEAF_EDGE, lw=1.0, alpha=0.7 * max(weight, 0.15), zorder=1)

    # Edges between copies.
    for index, copy in enumerate(COPIES):
        if copy["parent"] is None:
            continue
        m = merge_progress(t, copy["level"])
        alpha = growth(t, copy["level"]) * (1.0 if copy["first_edge"] else 1.0 - m)
        if alpha < 0.01:
            continue
        parent, child = state[copy["parent"]], state[index]
        ax.plot(*zip(parent["pos"], child["pos"]), color=child["color"], lw=1.7,
                alpha=0.7 * alpha, zorder=2, solid_capstyle="round")

    for pos, color in dots:
        for scale, alpha in [(3.0, 0.08), (1.8, 0.2), (1.0, 0.95)]:
            ax.add_patch(plt.Circle(pos, 0.008 * scale, color=color, alpha=alpha, lw=0, zorder=5))

    for index, node in enumerate(state):
        if node["alpha"] <= 0.01 or node["r"] <= 0.001:
            continue
        glow = 0.25 + 0.75 * flash[index]
        for scale, alpha in [(2.4, 0.05), (1.7, 0.1), (1.3, 0.18)]:
            ax.add_patch(plt.Circle(node["pos"], node["r"] * scale, color=node["color"],
                                    alpha=alpha * glow * node["alpha"], lw=0, zorder=3))
        core = lerp(node["color"], FLASH[0], FLASH[1] * flash[index])
        ax.add_patch(plt.Circle(node["pos"], node["r"], facecolor=lerp(to_rgb(BACKGROUND), core, 0.35),
                                edgecolor=core, lw=2.0, alpha=node["alpha"], zorder=4))

    for leaf, (band, _) in enumerate(LEAVES):
        color, pos = BAND_COLORS[band], (LEAF_X[leaf] * WIDTH, LEAF_Y)
        ax.add_patch(plt.Circle(pos, 0.03, color=color, alpha=0.06 + 0.3 * leaf_flash[leaf], lw=0, zorder=3))
        ax.add_patch(plt.Circle(pos, 0.014, color=color, alpha=0.55 + 0.45 * leaf_flash[leaf], lw=0, zorder=4))


def render(theme):
    apply_theme(theme)
    out = OUT_DIR / f"reactive_circuits_{theme}.gif"
    events = update_events()
    fig = plt.figure(figsize=(12, 5), dpi=100, facecolor=BACKGROUND)
    ax = fig.add_axes([0, 0, 1, 1], facecolor=BACKGROUND)

    with tempfile.TemporaryDirectory() as tmp:
        video = Path(tmp) / "animation.mp4"
        writer = FFMpegWriter(fps=FPS, codec="libx264", extra_args=["-pix_fmt", "yuv420p"])
        with writer.saving(fig, str(video), dpi=100):
            for frame in range(int(LOOP_S * FPS)):
                draw_frame(ax, frame / FPS, events)
                writer.grab_frame(facecolor=BACKGROUND)
        plt.close(fig)
        palette = Path(tmp) / "palette.png"
        scale = f"fps={FPS},scale=720:-1:flags=lanczos"
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(video),
                        "-vf", f"{scale},palettegen=max_colors=64:stats_mode=diff", str(palette)],
                       check=True)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(video), "-i", str(palette),
                        "-lavfi", f"{scale}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle",
                        "-loop", "0", str(out)], check=True)
    print(f"wrote {out} ({out.stat().st_size / 1e6:.1f} MB)")


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for theme in THEMES:
        render(theme)


if __name__ == "__main__":
    main()
