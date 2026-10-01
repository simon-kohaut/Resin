"""Renders docs/reactive_circuits_{dark,light}.gif with manim.

The scene follows `readme_animation_model`: four targets grow from flat
formulas into a shared DAG while leaf updates travel upward as pulses. Around
it, an update-rate axis labels each level with its frequency band, and live
counters show the number of nodes and edges.

Requires manim (https://www.manim.community), which needs Cairo and Pango but
no LaTeX, plus ffmpeg:  pip install manim
Run with: python scripts/render_readme_animation_manim.py [dark|light ...]
"""

import sys
import tempfile
from pathlib import Path

import numpy as np
from manim import (
    Circle, DashedLine, Line, Scene, Text, ValueTracker, VGroup, VMobject, linear, tempconfig,
)

sys.path.insert(0, str(Path(__file__).resolve().parent))
import readme_animation_model as model  # noqa: E402

OUT_DIR = Path(__file__).resolve().parent.parent / "docs"
FONT = "Menlo"

# Model coordinates (x in [0, WIDTH], y in [0, 1]) to the 12 x 5 manim frame.
SCALE = 3.85
ORIGIN = np.array([-4.55, -2.35, 0.0])


def point(p):
    return ORIGIN + np.array([p[0] * SCALE, p[1] * SCALE, 0.0])


def set_line(line, start, end, opacity):
    start, end = point(start), point(end)
    if np.linalg.norm(end - start) < 1e-4:
        end = start + np.array([1e-3, 0, 0])
        opacity = 0.0
    line.put_start_and_end_on(start, end)
    line.set_stroke(opacity=opacity)


def set_circle(circle, center, radius, opacity, fill=None):
    circle.scale_to_fit_width(max(2 * radius * SCALE, 1e-3))
    circle.move_to(point(center))
    circle.set_stroke(opacity=opacity)
    circle.set_fill(opacity=opacity if fill is None else fill)


class ReadmeScene(Scene):
    theme_name = "dark"

    def construct(self):
        theme = model.Theme(self.theme_name)
        events = model.update_events()
        clock = ValueTracker(0.0)
        hexed = model.to_hex

        def label(text, size=13, color=theme.text, **kwargs):
            return Text(text, font=FONT, font_size=size, color=color, **kwargs)

        # Update-rate axis: one tick per level, colored like its band.
        axis_x = ORIGIN[0] - 0.45
        top, bottom = point([0, model.LEVEL_Y[0]])[1], point([0, model.LEAF_Y])[1]
        self.add(Line([axis_x, bottom, 0], [axis_x, top + 0.25, 0], stroke_width=1.2,
                      color=theme.text, stroke_opacity=0.6))
        title = label("update rate", size=12).rotate(np.pi / 2)
        title.move_to([axis_x - 0.85, (top + bottom) / 2, 0])
        self.add(title)
        ticks, guides = {}, {}
        for level, band in enumerate(model.BANDS):
            y = point([0, model.LEVEL_Y[level]])[1]
            color = hexed(theme.bands[band])
            tick = VMobject()
            tick.add(Line([axis_x - 0.08, y, 0], [axis_x + 0.08, y, 0], stroke_width=1.5, color=color))
            tick.add(label(model.BAND_HZ[band], size=12, color=color).next_to([axis_x - 0.1, y, 0], direction=[-1, 0, 0], buff=0.08))
            ticks[level] = tick
            guides[level] = DashedLine(
                [axis_x + 0.15, y, 0], [point([model.WIDTH, 0])[0] + 0.1, y, 0],
                dash_length=0.06, stroke_width=1.0, color=theme.guide)
            self.add(guides[level], tick)
        self.add(label("inputs", size=12).next_to([axis_x - 0.1, bottom, 0], direction=[-1, 0, 0], buff=0.08))

        # Header and live counters.
        self.add(label("reactive circuit", size=14).to_corner([-1, 1, 0], buff=0.25))
        readouts = {}
        for row, key in enumerate(["nodes", "edges"]):
            name = label(key, size=12).move_to([4.05, 2.2 - row * 0.32, 0], aligned_edge=[-1, 0, 0])
            self.add(name)
            readouts[key] = label("", size=12, color=hexed(theme.targets[1]))
        text_cache = {}

        def readout(key, value, row):
            if (key, value) not in text_cache:
                text_cache[key, value] = label(value, size=12, color=hexed(theme.targets[0])).move_to(
                    [5.85, 2.2 - row * 0.32, 0], aligned_edge=[1, 0, 0])
            readouts[key].become(text_cache[key, value])

        # Circuit mobjects, created once and updated every frame.
        first = model.frame(0.0, events, theme)
        leaf_lines = [Line(stroke_width=1.1, color=theme.leaf_edge) for _ in first.leaf_edges]
        node_lines = [Line(stroke_width=2.2) for _ in first.node_edges]
        glows = [[Circle(stroke_width=0) for _ in range(3)] for _ in first.nodes]
        cores = [Circle(stroke_width=2.6) for _ in first.nodes]
        leaf_glows = [Circle(stroke_width=0) for _ in model.LEAVES]
        leaf_dots = [Circle(stroke_width=0) for _ in model.LEAVES]
        pool = 80
        trails = [Line(stroke_width=2.4) for _ in range(pool)]
        heads = [Circle(stroke_width=0) for _ in range(pool)]
        head_glows = [Circle(stroke_width=0) for _ in range(pool)]
        # Manim only redraws mobjects that move, so everything that changes
        # lives in one group that carries the updater.
        dynamic = VGroup(*guides.values(), *ticks.values())
        for group in (leaf_lines, node_lines, [g for gs in glows for g in gs], cores,
                      trails, head_glows, heads, leaf_glows, leaf_dots, readouts.values()):
            dynamic.add(*group)

        def redraw(_):
            t = clock.get_value() % model.LOOP_S
            f = model.frame(t, events, theme)
            for level, tick in ticks.items():
                opacity = 1.0 if level == 0 else f.level_growth[level]
                tick.set_opacity(opacity)
                guides[level].set_stroke(opacity=0.9 * opacity)
            for line, (start, end, alpha) in zip(leaf_lines, f.leaf_edges):
                set_line(line, start, end, 0.75 * alpha)
            for line, (start, end, color, alpha) in zip(node_lines, f.node_edges):
                line.set_stroke(color=hexed(color))
                set_line(line, start, end, 0.8 * alpha)
            for node, glow, core in zip(f.nodes, glows, cores):
                visible = node["alpha"] if node["r"] > 1e-3 else 0.0
                bright = model.lerp(node["color"], theme.flash_color, theme.flash_mix * node["flash"])
                strength = 0.25 + 0.75 * node["flash"]
                for circle, (scale, alpha) in zip(glow, [(2.4, 0.06), (1.7, 0.11), (1.3, 0.2)]):
                    circle.set_fill(color=hexed(node["color"]))
                    set_circle(circle, node["pos"], node["r"] * scale, 0.0, alpha * strength * visible)
                core.set_stroke(color=hexed(bright))
                core.set_fill(color=hexed(model.lerp(model.rgb(theme.background), bright, 0.35)))
                set_circle(core, node["pos"], node["r"], visible)
            for leaf, (band, _) in enumerate(model.LEAVES):
                color, flash = hexed(theme.bands[band]), f.leaf_flash[leaf]
                leaf_glows[leaf].set_fill(color=color)
                set_circle(leaf_glows[leaf], model.LEAF_POS[leaf], 0.03, 0.0, 0.06 + 0.3 * flash)
                leaf_dots[leaf].set_fill(color=color)
                set_circle(leaf_dots[leaf], model.LEAF_POS[leaf], 0.018, 0.0, 0.75 + 0.25 * flash)
            for i in range(pool):
                if i < len(f.pulses):
                    head, tail, color = f.pulses[i]
                    trails[i].set_stroke(color=hexed(color))
                    set_line(trails[i], tail, head, 0.55)
                    for circle, radius, alpha in [(head_glows[i], 0.02, 0.25), (heads[i], 0.008, 1.0)]:
                        circle.set_fill(color=hexed(color))
                        set_circle(circle, head, radius, 0.0, alpha)
                else:
                    trails[i].set_stroke(opacity=0.0)
                    heads[i].set_fill(opacity=0.0)
                    head_glows[i].set_fill(opacity=0.0)
            readout("nodes", f"{f.node_count:.0f}", 0)
            readout("edges", f"{f.edge_count:.0f}", 1)

        dynamic.add_updater(redraw)
        self.add(dynamic)
        redraw(dynamic)
        self.play(clock.animate.set_value(model.LOOP_S), run_time=model.LOOP_S, rate_func=linear)


def render(theme, gif_fps=12, gif_width=800):
    with tempfile.TemporaryDirectory() as tmp:
        config = {
            "pixel_width": 1440, "pixel_height": 600, "frame_width": 12, "frame_height": 5,
            "frame_rate": 30, "background_color": model.THEMES[theme]["background"],
            "media_dir": tmp, "output_file": "readme", "disable_caching": True,
            "progress_bar": "none", "verbosity": "WARNING",
        }
        with tempconfig(config):
            scene = type(f"ReadmeScene_{theme}", (ReadmeScene,), {"theme_name": theme})()
            scene.render()
            video = Path(scene.renderer.file_writer.movie_file_path)
            out = OUT_DIR / f"reactive_circuits_{theme}.gif"
            model.video_to_gif(video, out, gif_fps, gif_width)
            # Keep the full-quality video next to the scratch output for other uses.
            print(f"wrote {out} ({out.stat().st_size / 1e6:.1f} MB)")


if __name__ == "__main__":
    for theme in sys.argv[1:] or list(model.THEMES):
        render(theme)
