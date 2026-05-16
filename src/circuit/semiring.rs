use ndarray::{ArcArray1, Array1, ArrayView1};

/// A commutative semiring over f64 vectors, parameterised by an internal
/// representation (e.g. log-space for probability semirings).
///
/// The semiring algebra has:
///   - An additive identity (`zero`) and operation (`⊕`, used to sum over minterms)
///   - A multiplicative identity (`one`) and operation (`⊗`, used to multiply literals in a minterm)
///   - An `encode` step (raw input → internal repr) and `decode` step (internal repr → probability)
///
/// Implementations must be zero-sized marker types; all state lives in the
/// `SumAcc` associated type, which is stack-allocated per `evaluate` call.
pub trait Semiring: Clone + Send + Sync + 'static {
    /// Heap-allocated accumulator for the vectorised ⊕ across all minterms.
    /// Keeping it as an associated type lets each semiring own exactly the
    /// buffers it needs (e.g. two arrays for logsumexp, one for max/min).
    type SumAcc;

    /// Additive identity in the internal representation.
    fn zero() -> f64;
    /// Multiplicative identity in the internal representation.
    fn one() -> f64;

    /// Convert a raw input probability into the semiring's internal representation.
    fn encode(p: f64) -> f64;
    /// Convert internal representation back to a probability for external output.
    fn decode(v: f64) -> f64;

    /// Semiring negation in the encoded space: returns the encoding of `1 − decode(v)`.
    ///
    /// Used to produce the complementary leaf in a `Category` pair: given an
    /// encoded probability `v = S::encode(p)`, `negate(v)` returns `S::encode(1 − p)`.
    fn negate(v: f64) -> f64;

    /// In-place vectorised semiring product: `acc = acc ⊗ rhs`.
    /// Called once per literal when accumulating a minterm.
    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>);

    /// Allocate a fresh additive accumulator initialised to ⊕-identity (`zero`).
    fn sum_new(n: usize) -> Self::SumAcc;
    /// Fold one minterm value into the accumulator: `acc = acc ⊕ term`.
    fn sum_step(acc: &mut Self::SumAcc, term: &Array1<f64>);
    /// Finalise the accumulator into the result vector.
    fn sum_finish(acc: Self::SumAcc) -> Array1<f64>;

    /// Build the initial encoded storage for a new leaf.
    ///
    /// The default applies `encode` element-wise, which is correct for all
    /// scalar semirings.  `ProbGradient` overrides this to plant a gradient
    /// seed of `1.0` at position `leaf_index + 1`.
    fn encode_leaf_vec(value: ArrayView1<f64>, leaf_index: usize) -> Array1<f64> {
        let _ = leaf_index;
        value.mapv(Self::encode)
    }

    /// Reset a minterm accumulator to the ⊗-identity before processing a new row.
    ///
    /// Default fills every element with `one()`.  `ProbGradient` overrides this
    /// because its identity is `[1, 0, ..., 0]`, not a uniform scalar.
    fn reset_term(term: &mut Array1<f64>) {
        term.fill(Self::one());
    }

    /// Override the circuit's `value_size` based on the number of leaves.
    ///
    /// Returns `None` (the default) to keep the caller-supplied `value_size`.
    /// `ProbGradient` returns `Some(1 + n_leaves)` so that `Resin::compile`
    /// can size the gradient vector automatically.
    fn auto_value_size(n_leaves: usize) -> Option<usize> {
        let _ = n_leaves;
        None
    }

    /// Assert that the caller-supplied `value_size` (= batch size) is
    /// compatible with this semiring.  The default is permissive.
    /// `ProbGradient` overrides this to reject batch sizes other than 1.
    fn validate_value_size(value_size: usize) {
        let _ = value_size;
    }

    /// Expand an externally-supplied raw value to the semiring's internal
    /// `value_size` before encoding.
    ///
    /// The default is a no-op — the value is forwarded unchanged.
    /// `ProbGradient` overrides this: its internal `value_size` is `1 + n_leaves`
    /// while external writers always supply a 1-element vector, so the override
    /// places `value[0]` at slot 0 and zeros out the gradient slots.
    fn expand_input(value: ArcArray1<f64>, _value_size: usize) -> ArcArray1<f64> {
        value
    }
}

// ── LogProb: (ℝ∪{-∞}, logsumexp, +, -∞, 0) ──────────────────────────────────
//
// Internal representation: log-probabilities.
// ⊗ = addition in log-space (= multiplication of probabilities)
// ⊕ = numerically-stable logsumexp (= addition of probabilities)

#[derive(Clone)]
pub struct LogProb;

impl Semiring for LogProb {
    /// Three pre-allocated buffers: (running_max, running_sum, delta).
    /// Avoids any per-minterm heap allocation during evaluation.
    type SumAcc = (Array1<f64>, Array1<f64>, Array1<f64>);

    fn zero() -> f64 { f64::NEG_INFINITY }
    fn one()  -> f64 { 0.0 }
    fn encode(p: f64) -> f64 { p.ln() }
    fn decode(v: f64) -> f64 { v.exp() }
    fn negate(v: f64) -> f64 { (1.0 - v.exp()).ln() } // ln(1 − exp(v))

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        *acc += &rhs; // log(a · b) = log a + log b
    }

    fn sum_new(n: usize) -> Self::SumAcc {
        (
            Array1::from_elem(n, f64::NEG_INFINITY),
            Array1::<f64>::zeros(n),
            Array1::<f64>::zeros(n),
        )
    }

    fn sum_step((running_max, running_sum, delta): &mut Self::SumAcc, term: &Array1<f64>) {
        // Online logsumexp — one pass, no extra allocation.
        //
        // Invariant after k steps:
        //   running_max[i] = max of term[i] seen so far
        //   running_sum[i] = Σ exp(term[i] - running_max[i])
        ndarray::Zip::from(&mut *delta)
            .and(&mut *running_max)
            .and(term.view())
            .for_each(|d, m, &v| {
                let new_m = m.max(v);
                // When old max was -∞ the rescaling factor is 0, not NaN.
                *d = if *m == f64::NEG_INFINITY { 0.0 } else { (*m - new_m).exp() };
                *m = new_m;
            });
        *running_sum *= &*delta;
        ndarray::Zip::from(&mut *running_sum)
            .and(term.view())
            .and(running_max.view())
            .for_each(|s, &lv, &m| {
                // When new max is still -∞ the term is zero-probability; skip.
                if m > f64::NEG_INFINITY {
                    *s += (lv - m).exp();
                }
            });
    }

    fn sum_finish((mut running_max, mut running_sum, _): Self::SumAcc) -> Array1<f64> {
        running_sum.mapv_inplace(f64::ln);
        running_max += &running_sum;
        running_max
    }
}

// ── MaxProduct / MPE: (ℝ∪{-∞}, max, +, -∞, 0) in log-space ─────────────────
//
// Most-Probable-Explanation semiring.  Same encoding and product as LogProb,
// but the sum over minterms becomes a max instead of logsumexp.

#[derive(Clone)]
pub struct MaxProduct;

impl Semiring for MaxProduct {
    type SumAcc = Array1<f64>;

    fn zero() -> f64 { f64::NEG_INFINITY }
    fn one()  -> f64 { 0.0 }
    fn encode(p: f64) -> f64 { p.ln() }
    fn decode(v: f64) -> f64 { v.exp() }
    fn negate(v: f64) -> f64 { (1.0 - v.exp()).ln() } // ln(1 − exp(v))

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        *acc += &rhs; // same log-space product as LogProb
    }

    fn sum_new(n: usize) -> Self::SumAcc {
        Array1::from_elem(n, f64::NEG_INFINITY)
    }

    fn sum_step(acc: &mut Self::SumAcc, term: &Array1<f64>) {
        ndarray::Zip::from(acc.view_mut())
            .and(term.view())
            .for_each(|m, &v| *m = m.max(v));
    }

    fn sum_finish(acc: Self::SumAcc) -> Array1<f64> { acc }
}

// ── Fuzzy: ([0,1], max, min, 0, 1) ───────────────────────────────────────────
//
// Łukasiewicz / Zadeh fuzzy logic.
// ⊗ = min (fuzzy AND),  ⊕ = max (fuzzy OR).
// Values live in [0, 1].

#[derive(Clone)]
pub struct Fuzzy;

impl Semiring for Fuzzy {
    type SumAcc = Array1<f64>;

    fn zero() -> f64 { 0.0 }
    fn one()  -> f64 { 1.0 }
    fn encode(p: f64) -> f64 { p }
    fn decode(v: f64) -> f64 { v }
    fn negate(v: f64) -> f64 { 1.0 - v }

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        ndarray::Zip::from(acc.view_mut())
            .and(rhs)
            .for_each(|a, &b| *a = a.min(b));
    }

    fn sum_new(n: usize) -> Self::SumAcc { Array1::zeros(n) }

    fn sum_step(acc: &mut Self::SumAcc, term: &Array1<f64>) {
        ndarray::Zip::from(acc.view_mut())
            .and(term.view())
            .for_each(|a, &v| *a = a.max(v));
    }

    fn sum_finish(acc: Self::SumAcc) -> Array1<f64> { acc }
}

// ── Boolean: ({0, 1}, ∨, ∧, 0, 1) ───────────────────────────────────────────
//
// Classical satisfiability / model counting over {0.0, 1.0}.
// ⊗ = AND (multiplication),  ⊕ = OR (max on {0,1}).
// Values outside {0, 1} are snapped to {0, 1} by encode.

#[derive(Clone)]
pub struct Boolean;

impl Semiring for Boolean {
    type SumAcc = Array1<f64>;

    fn zero() -> f64 { 0.0 }
    fn one()  -> f64 { 1.0 }
    fn encode(p: f64) -> f64 { if p > 0.0 { 1.0 } else { 0.0 } }
    fn decode(v: f64) -> f64 { v }
    fn negate(v: f64) -> f64 { 1.0 - v } // 0 ↔ 1

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        // AND: 0 if either factor is 0, else 1
        ndarray::Zip::from(acc.view_mut())
            .and(rhs)
            .for_each(|a, &b| *a *= b);
    }

    fn sum_new(n: usize) -> Self::SumAcc { Array1::zeros(n) }

    fn sum_step(acc: &mut Self::SumAcc, term: &Array1<f64>) {
        // OR: max on {0, 1}
        ndarray::Zip::from(acc.view_mut())
            .and(term.view())
            .for_each(|a, &v| *a = a.max(v));
    }

    fn sum_finish(acc: Self::SumAcc) -> Array1<f64> { acc }
}

// ── ProbGradient: forward-mode autodiff over [0,1] ───────────────────────────

/// Forward-mode automatic differentiation semiring.
///
/// Computes WMC and all partial derivatives `∂WMC/∂xᵢ` in a single circuit
/// pass.  The result vector has layout `[WMC, ∂WMC/∂x₀, …, ∂WMC/∂xₙ₋₁]`
/// where each `xᵢ` is the probability of circuit leaf `i`, treated as an
/// independent parameter.  `value_size` is set automatically to `1 + n_leaves`
/// by `Resin::compile`.
///
/// # Mapping gradients back to network outputs
///
/// Each `result[i+1]` is the gradient w.r.t. the probability of circuit leaf
/// `i` as a free variable.  How you use these depends on your network:
///
/// **One network output per leaf** (the general case): use each `result[i+1]`
/// directly as the gradient for that output.  This covers k-class categories
/// `{dog, cat, horse}` where each class probability is an independent output.
///
/// **One network output driving a `Category` pair** (binary complement): a
/// single scalar output `p` feeds both a positive leaf (probability `p`) and a
/// negative leaf (probability `1−p`).  Because the circuit treats them as
/// independent parameters, the chain rule must be applied on the consumer side:
///
/// ```text
/// net_grad = result[pos+1] − result[neg+1]
/// ```
///
/// The subtraction supplies the `d(1−p)/dp = −1` factor.  With two independent
/// neurons feeding the pair (neither constrained to sum to one), use the two
/// gradient slots separately without combining.
#[derive(Clone)]
pub struct ProbGradient;

impl Semiring for ProbGradient {
    type SumAcc = Array1<f64>;

    fn zero() -> f64 { 0.0 }
    fn one()  -> f64 { 1.0 }
    fn encode(p: f64) -> f64 { p }
    fn decode(v: f64) -> f64 { v }
    fn negate(v: f64) -> f64 { 1.0 - v }

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        let p_acc = acc[0];
        let p_rhs = rhs[0];
        // Product rule: d(p·q)/dxᵢ = p·(dq/dxᵢ) + q·(dp/dxᵢ)
        for j in 1..acc.len() {
            acc[j] = p_acc * rhs[j] + p_rhs * acc[j];
        }
        acc[0] = p_acc * p_rhs;
    }

    fn sum_new(n: usize) -> Array1<f64> { Array1::zeros(n) }
    fn sum_step(acc: &mut Array1<f64>, term: &Array1<f64>) { *acc += term; }
    fn sum_finish(acc: Array1<f64>) -> Array1<f64> { acc }

    fn encode_leaf_vec(value: ArrayView1<f64>, leaf_index: usize) -> Array1<f64> {
        let p = value[0];
        let mut encoded = Array1::zeros(value.len());
        encoded[0] = p;
        if leaf_index + 1 < encoded.len() {
            encoded[leaf_index + 1] = 1.0;
        }
        encoded
    }

    fn reset_term(term: &mut Array1<f64>) {
        term.fill(0.0);
        term[0] = 1.0;
    }

    fn auto_value_size(n_leaves: usize) -> Option<usize> {
        Some(1 + n_leaves)
    }

    fn validate_value_size(value_size: usize) {
        assert!(
            value_size == 1,
            "ProbGradient does not support batching (value_size must be 1, got {})",
            value_size
        );
    }

    fn expand_input(value: ArcArray1<f64>, value_size: usize) -> ArcArray1<f64> {
        if value.len() < value_size {
            let mut expanded = Array1::zeros(value_size);
            expanded[0] = value[0];
            expanded.into_shared()
        } else {
            value
        }
    }
}
