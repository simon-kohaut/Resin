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

    /// Decode a whole result vector. Defaults to elementwise `decode`; semirings
    /// with non-probability blocks (e.g. `CriticalExplanation`'s `tag`) override it.
    fn decode_vec(mut v: Array1<f64>) -> Array1<f64> {
        v.mapv_inplace(Self::decode);
        v
    }

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

/// Clamp numerical overshoot (e.g. from interpolation) into `[0, 1]` before `ln`.
/// NaN is passed through so genuine upstream failures still surface.
#[inline]
fn clamp_unit(p: f64) -> f64 {
    if p.is_nan() {
        p
    } else {
        p.clamp(0.0, 1.0)
    }
}

#[derive(Clone)]
pub struct LogProb;

impl Semiring for LogProb {
    /// Three pre-allocated buffers: (running_max, running_sum, delta).
    /// Avoids any per-minterm heap allocation during evaluation.
    type SumAcc = (Array1<f64>, Array1<f64>, Array1<f64>);

    fn zero() -> f64 {
        f64::NEG_INFINITY
    }
    fn one() -> f64 {
        0.0
    }
    fn encode(p: f64) -> f64 {
        clamp_unit(p).ln()
    }
    fn decode(v: f64) -> f64 {
        v.exp()
    }
    fn negate(v: f64) -> f64 {
        clamp_unit(-v.exp_m1()).ln()
    } // ln(1 − exp(v))

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
                *d = if *m == f64::NEG_INFINITY {
                    0.0
                } else {
                    (*m - new_m).exp()
                };
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

    fn zero() -> f64 {
        f64::NEG_INFINITY
    }
    fn one() -> f64 {
        0.0
    }
    fn encode(p: f64) -> f64 {
        clamp_unit(p).ln()
    }
    fn decode(v: f64) -> f64 {
        v.exp()
    }
    fn negate(v: f64) -> f64 {
        clamp_unit(-v.exp_m1()).ln()
    } // ln(1 − exp(v))

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

    fn sum_finish(acc: Self::SumAcc) -> Array1<f64> {
        acc
    }
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

    fn zero() -> f64 {
        0.0
    }
    fn one() -> f64 {
        1.0
    }
    fn encode(p: f64) -> f64 {
        p
    }
    fn decode(v: f64) -> f64 {
        v
    }
    fn negate(v: f64) -> f64 {
        1.0 - v
    }

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        ndarray::Zip::from(acc.view_mut())
            .and(rhs)
            .for_each(|a, &b| *a = a.min(b));
    }

    fn sum_new(n: usize) -> Self::SumAcc {
        Array1::zeros(n)
    }

    fn sum_step(acc: &mut Self::SumAcc, term: &Array1<f64>) {
        ndarray::Zip::from(acc.view_mut())
            .and(term.view())
            .for_each(|a, &v| *a = a.max(v));
    }

    fn sum_finish(acc: Self::SumAcc) -> Array1<f64> {
        acc
    }
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

    fn zero() -> f64 {
        0.0
    }
    fn one() -> f64 {
        1.0
    }
    fn encode(p: f64) -> f64 {
        if p > 0.0 {
            1.0
        } else {
            0.0
        }
    }
    fn decode(v: f64) -> f64 {
        v
    }
    fn negate(v: f64) -> f64 {
        1.0 - v
    } // 0 ↔ 1

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        // AND: 0 if either factor is 0, else 1
        ndarray::Zip::from(acc.view_mut())
            .and(rhs)
            .for_each(|a, &b| *a *= b);
    }

    fn sum_new(n: usize) -> Self::SumAcc {
        Array1::zeros(n)
    }

    fn sum_step(acc: &mut Self::SumAcc, term: &Array1<f64>) {
        // OR: max on {0, 1}
        ndarray::Zip::from(acc.view_mut())
            .and(term.view())
            .for_each(|a, &v| *a = a.max(v));
    }

    fn sum_finish(acc: Self::SumAcc) -> Array1<f64> {
        acc
    }
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

    fn zero() -> f64 {
        0.0
    }
    fn one() -> f64 {
        1.0
    }
    fn encode(p: f64) -> f64 {
        p
    }
    fn decode(v: f64) -> f64 {
        v
    }
    fn negate(v: f64) -> f64 {
        1.0 - v
    }

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        let p_acc = acc[0];
        let p_rhs = rhs[0];
        // Product rule: d(p·q)/dxᵢ = p·(dq/dxᵢ) + q·(dp/dxᵢ)
        for j in 1..acc.len() {
            acc[j] = p_acc * rhs[j] + p_rhs * acc[j];
        }
        acc[0] = p_acc * p_rhs;
    }

    fn sum_new(n: usize) -> Array1<f64> {
        Array1::zeros(n)
    }
    fn sum_step(acc: &mut Array1<f64>, term: &Array1<f64>) {
        *acc += term;
    }
    fn sum_finish(acc: Array1<f64>) -> Array1<f64> {
        acc
    }

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

// ── CriticalExplanation: (P, v, tag, w) in log-space ─────────────────────────
//
// Per cell, tracks the marginal probability P (as `LogProb`), the MPE
// probability v (as `MaxProduct`), and a witness: `tag`, the index of the
// least-probable leaf within the most probable explanation, and `w`, that
// leaf's probability. `⊕` picks the whole (v, tag, w) triple of the side with
// larger v and never compares witnesses across branches otherwise, which keeps
// `⊗` distributive over `⊕`.
//
// Layout: value_size = 4 * n_cells, blocks [P | v | tag | w]; `tag` is not
// log-encoded.
#[derive(Clone)]
pub struct CriticalExplanation;

/// Order-independent tie-break: smaller `w` wins, then smaller leaf index.
#[inline]
fn weaker(t1: f64, w1: f64, t2: f64, w2: f64) -> (f64, f64) {
    if w1 < w2 || (w1 == w2 && t1 <= t2) {
        (t1, w1)
    } else {
        (t2, w2)
    }
}

/// `⊕` rule for `(v, tag, w)`: keep the triple with larger `v`; `weaker` only breaks exact ties.
#[inline]
fn mpe_pick(v1: f64, t1: f64, w1: f64, v2: f64, t2: f64, w2: f64) -> (f64, f64, f64) {
    if v1 > v2 {
        (v1, t1, w1)
    } else if v2 > v1 {
        (v2, t2, w2)
    } else {
        let (t, w) = weaker(t1, w1, t2, w2);
        (v1, t, w)
    }
}

impl Semiring for CriticalExplanation {
    type SumAcc = Array1<f64>;

    fn zero() -> f64 {
        f64::NEG_INFINITY
    }
    fn one() -> f64 {
        0.0
    }
    fn encode(p: f64) -> f64 {
        clamp_unit(p).ln()
    }
    fn decode(v: f64) -> f64 {
        v.exp()
    }
    fn negate(v: f64) -> f64 {
        clamp_unit(-v.exp_m1()).ln()
    }

    /// Exponentiates `P`, `v` and `w`; `tag` is a leaf index and stays as is.
    fn decode_vec(mut v: Array1<f64>) -> Array1<f64> {
        let n = v.len() / 4;
        v.slice_mut(ndarray::s![..2 * n]).mapv_inplace(f64::exp);
        v.slice_mut(ndarray::s![3 * n..]).mapv_inplace(f64::exp);
        v
    }

    fn mul_inplace(acc: &mut Array1<f64>, rhs: ArrayView1<f64>) {
        let n = acc.len() / 4;
        for i in 0..n {
            let (p1, v1, t1, w1) = (acc[i], acc[n + i], acc[2 * n + i], acc[3 * n + i]);
            let (p2, v2, t2, w2) = (rhs[i], rhs[n + i], rhs[2 * n + i], rhs[3 * n + i]);
            acc[i] = p1 + p2; // log(a·b) = log a + log b
            let v_new = v1 + v2;
            acc[n + i] = v_new;
            // An impossible branch (v = 0) carries no witness.
            let (t, w) = if v_new == f64::NEG_INFINITY {
                (f64::INFINITY, f64::INFINITY)
            } else {
                weaker(t1, w1, t2, w2)
            };
            acc[2 * n + i] = t;
            acc[3 * n + i] = w;
        }
    }

    fn sum_new(n: usize) -> Array1<f64> {
        // Additive identity per cell: (-inf, -inf, +inf, +inf), i.e. P = v = 0 and no witness.
        let quarter = n / 4;
        let mut acc = Array1::zeros(n);
        acc.slice_mut(ndarray::s![..2 * quarter]).fill(f64::NEG_INFINITY);
        acc.slice_mut(ndarray::s![2 * quarter..]).fill(f64::INFINITY);
        acc
    }
    fn sum_step(acc: &mut Array1<f64>, term: &Array1<f64>) {
        let n = acc.len() / 4;
        for i in 0..n {
            // P: pairwise logsumexp.
            let (a, b) = (acc[i], term[i]);
            acc[i] = if a == f64::NEG_INFINITY {
                b
            } else if b == f64::NEG_INFINITY {
                a
            } else if a >= b {
                a + (b - a).exp().ln_1p()
            } else {
                b + (a - b).exp().ln_1p()
            };
            let (v, t, w) = mpe_pick(
                acc[n + i],
                acc[2 * n + i],
                acc[3 * n + i],
                term[n + i],
                term[2 * n + i],
                term[3 * n + i],
            );
            acc[n + i] = v;
            acc[2 * n + i] = t;
            acc[3 * n + i] = w;
        }
    }
    fn sum_finish(acc: Array1<f64>) -> Array1<f64> {
        acc
    }

    fn encode_leaf_vec(value: ArrayView1<f64>, leaf_index: usize) -> Array1<f64> {
        let n = value.len() / 4;
        let mut encoded = Array1::zeros(value.len());
        let raw = value.slice(ndarray::s![..n]);
        for i in 0..n {
            let lp = clamp_unit(raw[i]).ln();
            encoded[i] = lp; // P = log p
            encoded[n + i] = lp; // v = log p
            encoded[2 * n + i] = leaf_index as f64; // tag = self
            encoded[3 * n + i] = lp; // w = log p
        }
        encoded
    }

    fn reset_term(term: &mut Array1<f64>) {
        // Multiplicative identity (log 1, log 1, +inf, +inf) = (0, 0, inf, inf).
        let n = term.len() / 4;
        term.fill(0.0); // P = v = log 1 = 0
        term.slice_mut(ndarray::s![2 * n..]).fill(f64::INFINITY); // tag, w
    }

    fn validate_value_size(value_size: usize) {
        assert!(
            value_size > 0 && value_size % 4 == 0,
            "CriticalExplanation's value_size must be 4 * n_cells (P, v, tag, w blocks), got {}",
            value_size
        );
    }

    fn expand_input(value: ArcArray1<f64>, value_size: usize) -> ArcArray1<f64> {
        let n = value_size / 4;
        debug_assert_eq!(
            value.len(),
            n,
            "CriticalExplanation expects a raw n_cells-length write"
        );
        let mut expanded = Array1::zeros(value_size);
        expanded.slice_mut(ndarray::s![..n]).assign(&value);
        expanded.into_shared()
    }
}

#[cfg(test)]
mod critical_explanation_axiom_tests {
    use super::*;

    fn quad(p: f64, v: f64, t: f64, w: f64) -> Array1<f64> {
        Array1::from(vec![p, v, t, w])
    }
    fn mul(a: &Array1<f64>, b: &Array1<f64>) -> Array1<f64> {
        let mut acc = a.clone();
        CriticalExplanation::mul_inplace(&mut acc, b.view());
        acc
    }
    fn add(a: &Array1<f64>, b: &Array1<f64>) -> Array1<f64> {
        let mut acc = a.clone();
        CriticalExplanation::sum_step(&mut acc, b);
        acc
    }

    /// Swapping operands on an exact tie (in `w` for ⊗, in `v` for ⊕) must not change the tag.
    #[test]
    fn test_commutative_on_exact_ties() {
        let x = quad(0.7, 0.7, 0.0, 0.3);
        let y = quad(0.4, 0.4, 1.0, 0.3); // tie in w with different tags
        assert_eq!(mul(&x, &y), mul(&y, &x), "mul_inplace must be commutative on ties");

        let a = quad(0.5, 0.6, 0.0, 0.4);
        let b = quad(0.2, 0.6, 1.0, 0.9); // tie in v with different tags
        assert_eq!(add(&a, &b), add(&b, &a), "sum_step must be commutative on v-ties");
    }

    #[test]
    fn test_associative() {
        let x = quad(0.9, 0.9, 0.0, 0.9);
        let y = quad(0.8, 0.8, 1.0, 0.8);
        let z = quad(0.9, 0.9, 2.0, 0.85);
        assert_eq!(mul(&mul(&x, &y), &z), mul(&x, &mul(&y, &z)), "mul_inplace assoc");
        assert_eq!(add(&add(&x, &y), &z), add(&x, &add(&y, &z)), "sum_step assoc");
    }

    /// `x ⊗ (y ⊕ z) = (x ⊗ y) ⊕ (x ⊗ z)`, which makes results invariant under `lift_leaf`/`drop_leaf`.
    #[test]
    fn test_distributive() {
        let x = quad(0.6, 0.6, 0.0, 0.6);
        let y = quad(0.5, 0.5, 1.0, 0.5);
        let z = quad(0.3, 0.3, 2.0, 0.3);
        let lhs = mul(&x, &add(&y, &z));
        let rhs = add(&mul(&x, &y), &mul(&x, &z));
        assert_eq!(lhs, rhs, "x*(y+z) must equal x*y+x*z");
    }

    /// Distributivity with `v` independent of `P`. `P` is compared up to rounding, `v`/`tag`/`w` exactly.
    #[test]
    fn test_distributive_v_independent_of_p() {
        let x = quad(0.9, 0.6, 0.0, 0.6);
        let y = quad(0.4, 0.5, 1.0, 0.5);
        let z = quad(0.1, 0.3, 2.0, 0.3);
        let lhs = mul(&x, &add(&y, &z));
        let rhs = add(&mul(&x, &y), &mul(&x, &z));
        assert!((lhs[0] - rhs[0]).abs() < 1e-12, "P: x*(y+z) must equal x*y+x*z, got {} vs {}", lhs[0], rhs[0]);
        assert_eq!(lhs[1], rhs[1], "v: x*(y+z) must equal x*y+x*z exactly");
        assert_eq!(lhs[2], rhs[2], "tag: x*(y+z) must equal x*y+x*z exactly");
        assert_eq!(lhs[3], rhs[3], "w: x*(y+z) must equal x*y+x*z exactly");
    }

    #[test]
    fn test_identities() {
        let mut one = Array1::zeros(4);
        CriticalExplanation::reset_term(&mut one);
        let zero = CriticalExplanation::sum_new(4);
        let x = quad(0.42, 0.42, 5.0, 0.42);

        assert_eq!(mul(&one, &x), x, "1*x = x");
        assert_eq!(mul(&x, &one), x, "x*1 = x");
        assert_eq!(add(&zero, &x), x, "0+x = x");
        assert_eq!(add(&x, &zero), x, "x+0 = x");
        assert_eq!(mul(&zero, &x), zero, "0*x = 0 exactly (annihilation, v/tag/w included)");
    }

    /// Certain leaves are never tagged: p=0 makes the branch's `v` zero so it
    /// never wins ⊕; p=1 has the largest possible `w` so it never wins ⊗.
    #[test]
    fn test_degenerate_leaf_never_wins() {
        let encode = |p: f64, idx| {
            CriticalExplanation::encode_leaf_vec(Array1::from_elem(4, p).view(), idx)
        };
        let certain_true = encode(1.0, 7);
        let certain_false = encode(0.0, 8);
        let uncertain = encode(0.5, 9);

        // Values below are log-encoded (except tag).
        let ln_half = 0.5_f64.ln();

        let live = mul(&certain_true, &uncertain);
        assert_eq!(live[0], ln_half);
        assert_eq!(live[1], ln_half);
        assert_eq!(live[2], 9.0, "certain leaf 7 must not be tagged as the weakest link");
        assert_eq!(live[3], ln_half);

        let dead = mul(&certain_false, &uncertain);
        assert_eq!(dead[1], f64::NEG_INFINITY);
        assert_eq!(dead[2], f64::INFINITY, "impossible branch must not leak a witness");
        assert_eq!(dead[3], f64::INFINITY);

        let summed = add(&dead, &live);
        assert_eq!(summed[1], ln_half);
        assert_eq!(summed[2], 9.0);
        assert_eq!(summed[3], ln_half);
    }
}
