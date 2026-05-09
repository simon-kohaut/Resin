# Resin — Reactive Signal Inference

**Resin** is a probabilistic logic programming language for building reactive inference pipelines over continuous, asynchronous data streams. 
Resin programs are compiled via Answer Set Programming (ASP) into **Reactive Circuits**: vectorised, self-adapting computation graphs that perform Weighted Model Counting (WMC) in real time.

The core library is written in Rust. A Python package (`pyresin`) is published to [PyPI](https://pypi.org/p/pyresin) and built with [Maturin](https://github.com/PyO3/maturin).

## The Resin language

A Resin program declares **sources** (incoming signals), **rules** (first-order logic), and **targets** (the quantities to infer).

### Source types

| Type | Declared with | ASP encoding |
|---|---|---|
| `Probability` | a value in `[0, 1]` | choice atom `{name}.` |
| `Boolean` | `true`/`false` | choice atom `{name}.` |
| `Density` | a continuous distribution | one choice per comparison threshold |
| `Number` | a scalar value | one choice per comparison threshold |

### Syntax

Here is an example Resin program for an autonomous aircraft system navigating an urban environment.

```prolog
# Source declarations
over(park)         <- source("/map/over/park", Probability).
distance(hospital) <- source("/map/distance/hospital", Density).
distance(airport)  <- source("/map/distance/airport", Density).
speed              <- source("/sensor/speed", Number).
flight_hours(w1)   <- source("/metrics/flight_hours/wing_1", Number).
flight_hours(w2)   <- source("/metrics/flight_hours/wing_2", Number).
flight_hours(w3)   <- source("/metrics/flight_hours/wing_3", Number).
flight_hours(w4)   <- source("/metrics/flight_hours/wing_4", Number).

# Propositional rules
permitted if over(park) and speed < 25.

# First-order rules
critical_infrastructure(hospital).
critical_infrastructure(airport).
safety_distance(T) if critical_infrastructure(T) and distance(T) > 100.

# Conditional probabilities and Noisy-OR over first-order instantiations
wing(w1). wing(w2). wing(w3). wing(w4).
needs_checkup(W) <- P(0.9) if flight_hours(W) > 100 and wing(W).
any_wing_needs_checkup if needs_checkup(W).

# Target that the program will be constrained on
safe if permitted and safety_distance(T) and not any_wing_needs_checkup.
safe -> target("/output/safe").
```

Rules supports variables (uppercase arguments, in the example above `W`, `T`) and conjunctions (`and`); disjunctions are implemented through multiple clauses.
Comparison literals (`<`, `>`) on `Number` and `Density` sources are automatically mapped to the respective leaves, which hold time-dependent truth values or CDF/SF probabilities.

## Installation

For employing Resin with Python, you can install the pre-compiled package via PyPI:

```bash
pip install pyresin
```

## Python API

### Compiling a model

```python
from resin import Resin

model = """
active <- source("/sensors/active", Boolean).
alarm  if active.
alarm  -> target("/output/alarm").
"""

resin = Resin.compile(model, value_size=1, verbose=False)
```

`value_size` sets the width of the internal value-space vector (e.g. number of particles or grid cells for vectorised evaluation).
This is helpful for running the same Resin program for many problem instances in parallel. 

### Writing signals

Both `make_writer(channel)` and `make_writer_for(atom)` return a correctly typed writer for the
declared source — the former looks up by IPC channel name, the latter by source atom name.

```python
# Boolean source — by channel name
bool_writer = resin.make_writer("/sensors/active")
bool_writer.write(True, timestamp=None)

# Probability source — by atom name
prob_writer = resin.make_writer_for("over(park)")
prob_writer.write([0.73], timestamp=None)

# Density source — pass distribution name and parameters
# Every time parameters are written, the density function may change
density_writer = resin.make_writer("/map/distance/hospital")
density_writer.write("normal", [[25.0], [5.0]], timestamp=None)
# Supported distributions: "normal", "lognormal", "exponential", "uniform"

# Number source for scalar comparison
number_writer = resin.make_writer_for("speed")
number_writer.write([12.5], timestamp=None)
```

### Reading outputs

```python
rc = resin.get_reactive_circuit()
results = rc.update()          # returns {target_name: [probability, ...]}
print(results["/output/alarm"])
```

### Reactive Circuit adaptation

The underlying circuit can adapt its structure in response to changing signal frequencies:

```python
rc.adapt(bin_size=0.1, number_bins=10)
```

Alternatively, leaves can be lifted or dropped at runtime, meaning we may manually indicate that a leaf's value changes more or less often than others:

```python
names = resin.get_names()
rc.lift_leaf(names.index("distance_vessel_gt_100"))
rc.drop_leaf(names.index("distance_vessel_gt_100"))
```

### Visualisation

The following allows inspecting the internal inference structure combined of the Reactive Circuit and each of its Algebraic Circuits.

```python
rc.to_combined_svg("circuit.svg")
```

## Building from source

Requirements: Rust toolchain, Clingo, Python ≥ 3.9, [Maturin](https://github.com/PyO3/maturin).

**macOS**
```bash
brew install clingo
export CLINGO_LIBRARY_PATH=$(brew --prefix clingo)/lib
maturin develop --release  # Optional for building the Python package
```

**Linux**
```bash
pip install clingo
CLINGO_DIR=$(python3 -c "import clingo, os; print(os.path.dirname(clingo.__file__))")
export CLINGO_LIBRARY_PATH="$CLINGO_DIR"
maturin develop --release  # Optional for building the Python package
```

**Run tests**
```bash
cargo test
```

## License

See [LICENSE.md](LICENSE.md).


## Citation

If you find our work useful, please consider citing the paper `Reactive Knowledge Representation and Asynchronous Reasoning`:

```
@article{kohaut2026reactive,
  title={Reactive Knowledge Representation and Asynchronous Reasoning},
  author={Kohaut, Simon and Flade, Benedict and Eggert, Julian and Kersting, Kristian and Dhami, Devendra Singh},
  journal={arXiv preprint arXiv:2602.05625},
  year={2026}
}
```
