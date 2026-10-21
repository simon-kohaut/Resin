# Python API

The `pyresin` package provides the `resin` module, with type stubs and docstrings for every class and method (`help(resin)`).

## Compiling a model

```python
from resin import Resin

model = """
active <- source("/sensors/active", Boolean).
alarm if active.
alarm -> target("/output/alarm").
"""

resin = Resin.compile(model, value_size=1, verbose=False)
```

`value_size` sets the number of values per source, e.g. the number of particles or grid cells for vectorised evaluation.
This is helpful for running the same Resin program for many problem instances in parallel.
The [semiring](semirings.md) is chosen with `semiring=`; `max_models` limits the number of stable models per target and raises `resin.TooManyModelsError` (a `RuntimeError`) when exceeded.
The syntax of the program itself is described in [The Resin Language](language.md).

## Writing signals

Both `make_writer(channel)` and `make_writer_for(atom)` return a correctly typed writer for the
declared source — the former looks up by IPC channel name, the latter by source atom name.

```python
# Boolean source — by channel name
bool_writer = resin.make_writer("/sensors/active")
bool_writer.write(True)

# Probability source — by atom name
prob_writer = resin.make_writer_for("over(park)")
prob_writer.write([0.73])

# Density source — pass distribution name and parameters
# Every time parameters are written, the density function may change
density_writer = resin.make_writer("/map/distance/hospital")
density_writer.write("normal", [[25.0], [5.0]])
# Supported distributions: "normal", "lognormal", "exponential", "uniform"

# Number source for scalar comparison
number_writer = resin.make_writer_for("speed")
number_writer.write([12.5])

# Categorical source — flat vector of class probabilities
cat_writer = resin.make_categorical_writer("/classifier/digit")
cat_writer.write([0.1, 0.6, 0.3])
```

Each `write` takes an optional `timestamp` in seconds (the current time by default), from which Resin estimates how often each source changes.

> [!IMPORTANT]
> Writers deliver their values asynchronously: give them a moment (e.g. `time.sleep(0.05)`) before an update that must include them.

## Updating targets

```python
rc = resin.get_reactive_circuit()

rc.update()       # {"/output/alarm": array([...])} for the targets that changed
rc.full_update()  # all targets, recomputing the whole circuit
```

`update()` only recomputes the parts of the circuit affected by new values, and only returns the targets that were recomputed.

## Reactive Circuit adaptation

The underlying circuit can adapt its structure in response to changing signal frequencies.
For example, to group source leaves in 0.1 Hz wide bins, each bin being separated into its own group of circuits, you can run:

```python
rc.adapt(bin_size=0.1, number_bins=10)
```

Alternatively, leaves can be lifted or dropped at runtime, meaning we may manually indicate that a leaf's value changes more or less often than others:

```python
names = resin.get_names()
rc.lift_leaf(names.index("alarm"))
rc.drop_leaf(names.index("raining"))
```

By default, adaptation builds a DAG: sub-circuits with identical formulas are merged, so targets of the same program grow into one another and share computation.
Pass `dag=False` to `adapt`, `lift_leaf` or `drop_leaf` to keep a separate tree per target instead, e.g., for comparison.

## Building circuits by hand

Reactive circuits can also be built directly from leaves and sums of products, for any semiring:

```python
from resin import ReactiveCircuit

rc = ReactiveCircuit(1, semiring="MaxProduct")
x, y, z = (rc.add_leaf([p], 0.0, name) for p, name in [(0.5, "x"), (0.4, "y"), (0.3, "z")])
rc.add_sum_product([[x, y], [z]], "t")  # t = x·y + z
rc.full_update()  # {"t": array([0.3])}
```

Leaves take `value_size` probabilities, both in `add_leaf` and `update_leaf`, for every semiring.
