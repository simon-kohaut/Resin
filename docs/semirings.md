# Semirings

Resin's inference algebra, and thereby the value which is computed per target, is selectable at compile time.
Every Resin program can be evaluated under a different semiring by passing `semiring=` to `Resin.compile` in Python, or by changing the type parameter `S` in `Resin::<S>::compile(...)` in Rust.

```python
from resin import Resin

# "LogProb" is the default; also "MaxProduct", "Fuzzy", "Boolean", "ProbGradient" or "CriticalExplanation"
resin = Resin.compile(code, value_size=1, semiring="LogProb")
result = resin.get_reactive_circuit().update()
# result["/output/safe"] contains the resulting value
```

## `LogProb` — standard probabilistic inference (default)

Computes the sum of probabilities of all satisfying worlds.

```
⊗ = product of probabilities  (log-space: addition)
⊕ = sum of probabilities      (log-space: numerically-stable logsumexp)
```

## `MaxProduct` — Most Probable Explanation

Computes the **single most-likely world**.
The sum over minterms becomes a max, so the circuit returns the probability of the highest-weight satisfying assignment rather than the marginal.

```
⊗ = product
⊕ = max
```

## `Fuzzy` — degree of truth

Evaluates the program under **Łukasiewicz / Zadeh fuzzy logic**, treating input probabilities as membership grades.

```
⊗ = min (fuzzy AND)
⊕ = max (fuzzy OR)
```

The result is the degree to which the target condition holds, dominated by the strongest single conjunction.
For example, a target satisfied by the three conjunctions `a·b`, `¬a·b` and `a·c` with `a = 0.8`, `b = 0.7` and `c = 0.3` holds to degree `max(min(0.8, 0.7), min(0.2, 0.7), min(0.8, 0.3)) = 0.7`.

## `Boolean` — satisfiability

Answers **"is the target satisfiable?"** by snapping all input probabilities to `{0, 1}` and evaluating with classical AND/OR.
Returns `1.0` if any world satisfies the target, `0.0` otherwise.

```
⊗ = AND   ⊕ = OR   encode: p > 0 → 1, else 0
```

## `ProbGradient` — forward-mode autodiff

Computes **probabilistic inference and all partial derivatives** using forward-mode automatic differentiation.
The result vector for a circuit with `n` leaves has layout:

```
[WMC, ∂WMC/∂x₀, ∂WMC/∂x₁, …, ∂WMC/∂xₙ₋₁]
```

> [!NOTE]
> Batched operations are not supported: `value_size` must be `1`, and the circuit's values grow to `1 + n` automatically.

Because `ProbGradient` returns the full Jacobian, it enables **gradient-based learning** of clause probabilities and of networks that feed sources, see [Learning](learning.md).

## `CriticalExplanation` — weakest link of the most probable explanation

Computes the marginal probability together with **the most probable explanation and its least-probable fact**.
Sources are written with `value_size` probabilities as usual, and the result vector has four blocks of `value_size` values each:

```
[P | v | tag | w]
```

- `P`: probability of the target (as in `LogProb`)
- `v`: probability of the most probable explanation (as in `MaxProduct`)
- `tag`: index of the least-probable circuit leaf within that explanation
- `w`: probability of that leaf

Because `⊕` decides by `v` alone, `⊗` distributes over `⊕` and the result is independent of how the circuit is restructured by adaptation.
