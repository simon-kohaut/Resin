# Resin — Reactive Signal Inference

[![CI](https://github.com/simon-kohaut/resin/actions/workflows/ci.yml/badge.svg)](https://github.com/simon-kohaut/resin/actions/workflows/ci.yml)
[![Release](https://github.com/simon-kohaut/resin/actions/workflows/pypi_release.yml/badge.svg)](https://github.com/simon-kohaut/resin/actions/workflows/pypi_release.yml)
[![PyPI version](https://img.shields.io/pypi/v/pyresin)](https://pypi.org/project/pyresin/)
[![Python versions](https://img.shields.io/pypi/pyversions/pyresin)](https://pypi.org/project/pyresin/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE.md)

**Resin** is a probabilistic first-order logic programming language for building reactive inference pipelines over continuous, asynchronous data streams. 
Resin programs are compiled via Answer Set Programming (ASP) into **Reactive Circuits**: vectorised, self-adapting computation graphs that perform Algebraic Model Counting (AMC) in real time.

The core library is written in Rust. A Python package (`pyresin`) is published to [PyPI](https://pypi.org/p/pyresin) and built with [Maturin](https://github.com/PyO3/maturin).

## Installation

For employing Resin with Python, you can install the pre-compiled package via PyPI:

```bash
pip install pyresin
```

To use Resin with Rust, you may clone this repository and build locally with `cargo`.

## The Resin language

A Resin program declares **sources** (incoming signals), **rules** (first-order logic), and **targets** (the quantities to infer).

### Source types

| Type | Declared with | ASP encoding |
|---|---|---|
| `Probability` | a value in `[0, 1]` | choice atom `{name}.` |
| `Boolean` | `true`/`false` | choice atom `{name}.` |
| `Density` | a continuous distribution | exactly-one choice over the intervals induced by the source's thresholds |
| `Number` | a scalar value | exactly-one choice over the intervals induced by the source's thresholds |
| `Categorical` | a vector of class probabilities | `1 { c₀ ; c₁ ; … } 1.` exactly-one constraint |

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
{sunny, raining}   <- source("/weather", Categorical).

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
safe if permitted and safety_distance(T) and not any_wing_needs_checkup and not raining.
safe -> target("/output/safe").
```

Rules supports variables (uppercase arguments, in the example above `W`, `T`) and conjunctions (`and`); disjunctions are implemented through multiple clauses.
Comparison literals (`<`, `>`) on `Number` and `Density` sources (ground atom left, constant literal value right) are derived from the exactly-one interval choice of their source: all thresholds registered for a source partition it into mutually exclusive intervals, so two comparisons on the same source (e.g. `s < 20` and `s < 30`) are never treated as independent.
Categorical sources provide probabilities for mutually exclusive ground atoms that are assumed to sum up to 1.

In Python, using the Resin code from above, inference can be run over one of the supported commutative semirings:

```python
from resin import Resin
semiring = "LogProb"  # default, otherwise use boolean, fuzzy, maxproduct, or probgradient
resin = Resin.compile(code, value_size=1, semiring=semiring)
result = resin.get_reactive_circuit().update()
# result["/output/safe"] contains resulting value
```

## Semirings

Resin's inference algebra, and thereby the value which is computed per target, is selectable at runtime.  
Every Resin program can be evaluated under a different semiring by changing the type parameter `S` in `Resin::<S>::compile(...)`.  

### `LogProb` — standard probabilistic inference (default)

Computes the sum of probabilities of all satisfying worlds.

```
⊗ = product of probabilities  (log-space: addition)
⊕ = sum of probabilities      (log-space: numerically-stable logsumexp)
```

### `MaxProduct` — Most Probable Explanation

Computes the **single most-likely world**.  
The sum over minterms becomes a max, so the circuit returns the probability of the highest-weight satisfying assignment rather than the marginal.

```
⊗ = product   
⊕ = max
```

### `Fuzzy` — degree of truth

Evaluates the program under **Łukasiewicz / Zadeh fuzzy logic**, treating input probabilities as membership grades.

```
⊗ = min (fuzzy AND)   
⊕ = max (fuzzy OR)
```

The result is the degree to which the target condition holds, dominated by the strongest single conjunction.  For the same proximity model: `max(min(0.8, 0.7), min(0.2, 0.7), min(0.8, 0.3)) = 0.7`.

### `Boolean` — satisfiability

Answers **"is the target satisfiable?"** by snapping all input probabilities to `{0, 1}` and evaluating with classical AND/OR.  Returns `1.0` if any world satisfies the target, `0.0` otherwise.

```
⊗ = AND   ⊕ = OR   encode: p > 0 → 1, else 0
```

### `ProbGradient` — forward-mode autodiff

Computes **probabilistic inference and all partial derivatives** using forward-mode automatic differentiation.
The result vector for a circuit with `n` leaves has layout:

```
[WMC, ∂WMC/∂x₀, ∂WMC/∂x₁, …, ∂WMC/∂xₙ₋₁]
```

Currently, no batched operations are supported, hence the `value_size` parameter is ignored and automatically set to `1 + n_parameters`.
Because `ProbGradient` returns the full Jacobian, it enables **gradient-based learning of leaf probabilities** directly inside Resin.  
With each `gradient_update()` call , the probability of the target is evaluated together with all gradients and can be used to tune program internal parameters:

```python
import time
from resin import Resin

# An example program for the safe deployment of a quadcopter
code = """
flight_hours(w1)   <- source("/metrics/flight_hours/wing_1", Number).
flight_hours(w2)   <- source("/metrics/flight_hours/wing_2", Number).
flight_hours(w3)   <- source("/metrics/flight_hours/wing_3", Number).
flight_hours(w4)   <- source("/metrics/flight_hours/wing_4", Number).

wing(w1). wing(w2). wing(w3). wing(w4).
needs_checkup(W) <- P(0.9) if flight_hours(W) > 100 and wing(W).
any_wing_needs_checkup if needs_checkup(W).

safe if any_wing_needs_checkup.
safe -> target("/output/safety").
"""

# Setup training for simple example program
# Make sure to use ProbGradient semiring for computing gradients alongside probabilities
resin = Resin.compile(code, semiring="ProbGradient")
reactive_circuit = resin.get_reactive_circuit()

# Set all wings' flight_hours above 100 so the condition is active
for channel in [
    "/metrics/flight_hours/wing_1",
    "/metrics/flight_hours/wing_2",
    "/metrics/flight_hours/wing_3",
    "/metrics/flight_hours/wing_4",
]:
    writer = resin.make_writer(channel)
    writer.write([200.0], timestamp=None)
time.sleep(0.05)

# Training parameters
ground_truth = 0.5
learning_rate = 0.1
for timestep in range(500):
    result = reactive_circuit.gradient_update()

    # Updated weights was not enough to invalidate any circuit
    # -> Training converged
    if not result:
        break

    # Get inference and gradient results
    # Gradients are dictionary from leaf_name -> gradient
    probability = result["/output/safety"]["probability"]
    gradients = result["/output/safety"]["gradients"]

    # Finish once Man Squared Error (MSE) is small enough
    if abs(probability - ground_truth) < 1e-3:
        print(f"Converged at step {timestep}: P(safe) = {probability:.4f}")
        break

    # Compute MSE and perform gradient descent step
    # We set parameters to "needs_checkup#0" to only fit the conditional 
    # probability of the first clause with that head
    mse = 2.0 * (probability - ground_truth)
    resin.fit_parameters(
        gradients, learning_rate, mse,
        parameters=["needs_checkup#0"], timestamp=float(timestep),
    )
```

**Gradient mapping for network outputs**

When leaf probabilities come from a neural network, the `gradients` dict provides the upstream values to feed into the network's own backward pass. 
You can access all gradients related to your source channel via `resin.source_gradients(channel_name)` or `resin.source_gradients_for(atom_name)`.

Note that you may have to combine gradients depending on your networks output layer, e.g., for a single output neuron that was used to provide a probability you need to compute `full_gradient = gradient[atom] - gradient[-atom]` to include the gradient on the negation.

## Python API

### Compiling a model

```python
from resin import Resin

model = """
active <- source("/sensors/active", Boolean).
alarm if active.
alarm -> target("/output/alarm").
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
bool_writer.write([True], timestamp=None)

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

# Categorical source — flat vector of class probabilities
cat_writer = resin.make_categorical_writer("/classifier/digit")
cat_writer.write([0.1, 0.6, 0.3], timestamp=None)
```

### Reactive Circuit adaptation

The underlying circuit can adapt its structure in response to changing signal frequencies:
For example, to group source leafs in 0.1Hz wide bins, each bin being separated into its own group of circuits, you can run:

```python
rc.adapt(bin_size=0.1, number_bins=10)
```

Alternatively, leaves can be lifted or dropped at runtime, meaning we may manually indicate that a leaf's value changes more or less often than others:

```python
names = resin.get_names()
rc.lift_leaf(names.index("alarm"))
rc.drop_leaf(names.index("raining"))
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
