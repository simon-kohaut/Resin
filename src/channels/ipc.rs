use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::circuit::leaf::update;
use crate::circuit::semiring::Semiring;
use crate::circuit::ReactiveCircuit;

use super::Vector;

type ComparisonChannel = (f64, bool, Sender<(Vector, f64)>);

/// Listens on an MPSC channel and writes received `(value, timestamp)` pairs
/// to a single leaf in the reactive circuit, optionally inverting the value.
#[derive(Clone)]
pub struct IpcReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>, // Keep handle to keep thread alive
}

/// Like `IpcReader` but writes to two leaves simultaneously: one with the
/// original value and one with `1 − value`.  Used for complementary leaf pairs.
#[derive(Clone)]
pub struct IpcDualReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>, // Keep handle to keep thread alive
}

/// Sends `(Vector, timestamp)` pairs to a channel via an MPSC sender.
/// The timestamp defaults to the current Unix time when `None` is supplied.
pub struct IpcWriter {
    sender: Sender<(Vector, f64)>,
}

/// Wraps an `IpcWriter` and sends the current value at a fixed `frequency`
/// (Hz) from a background thread.  The shared `value` can be updated
/// concurrently via `get_value_access`.  Call `start`/`stop` to control the
/// background thread; the thread is stopped automatically on `Drop`.
pub struct TimedIpcWriter {
    pub frequency: f64,
    value: Arc<Mutex<Vector>>,
    sender: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
    writer: IpcWriter,
}

impl IpcReader {
    /// Spawns a reader thread that forwards values from `receiver` to leaf
    /// `index`.  If `invert` is `true`, each value is replaced by `1 − value`.
    pub fn new<S: Semiring>(
        shared_reactive_circuit: Arc<Mutex<ReactiveCircuit<S>>>,
        index: u32,
        channel: &str,
        invert: bool,
        receiver: mpsc::Receiver<(Vector, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let handle = std::thread::spawn(move || {
            while let Ok(mut latest) = receiver.recv() {
                // Collapse any backlog that built up while we were busy —
                // leaves represent current state, so only the most recent
                // value matters and applying stale ones is wasted work.
                while let Ok(next) = receiver.try_recv() {
                    latest = next;
                }
                let (value, timestamp) = latest;
                let final_value = if invert {
                    Vector::ones(value.len()) - value
                } else {
                    value
                };
                update(
                    &mut shared_reactive_circuit.lock().unwrap(),
                    index,
                    final_value,
                    timestamp,
                );
            }
        });

        Ok(Self {
            topic: channel.to_owned(),
            _handle: Arc::new(handle),
        })
    }
}

impl IpcDualReader {
    /// Spawns a reader thread that writes each received value to leaf
    /// `index_normal` and `1 − value` to leaf `index_inverted` atomically
    /// (both updates hold the circuit lock together).
    pub fn new<S: Semiring>(
        shared_reactive_circuit: Arc<Mutex<ReactiveCircuit<S>>>,
        index_normal: u32,
        index_inverted: u32,
        channel: &str,
        receiver: mpsc::Receiver<(Vector, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let handle = std::thread::spawn(move || {
            while let Ok(mut latest) = receiver.recv() {
                // Collapse any backlog that built up while we were busy —
                // leaves represent current state, so only the most recent
                // value matters and applying stale ones is wasted work.
                while let Ok(next) = receiver.try_recv() {
                    latest = next;
                }
                let (value, timestamp) = latest;
                let inverted_value = Vector::ones(value.len()) - &*value;
                let mut circuit_guard = shared_reactive_circuit.lock().unwrap();
                update(&mut circuit_guard, index_normal, value.clone(), timestamp);
                update(
                    &mut circuit_guard,
                    index_inverted,
                    inverted_value,
                    timestamp,
                );
            }
        });

        Ok(Self {
            topic: channel.to_owned(),
            _handle: Arc::new(handle),
        })
    }
}

impl IpcWriter {
    /// Wraps `sender` in an `IpcWriter`.
    pub fn new(sender: Sender<(Vector, f64)>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { sender })
    }

    /// Sends `value` with `timestamp` (or the current Unix time if `None`).
    /// Send failures (e.g. disconnected receiver) are silently ignored.
    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let timestamp = timestamp.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        });

        let _ = self.sender.send((value, timestamp));
    }
}

impl TimedIpcWriter {
    /// Creates a new timed writer at the given `frequency` (Hz) with an
    /// initial `value`.  Call `start` to begin periodic transmission.
    pub fn new(
        frequency: f64,
        sender: Sender<(Vector, f64)>,
        value: Vector,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let writer = IpcWriter::new(sender)?;

        Ok(Self {
            frequency,
            value: Arc::new(Mutex::new(value)),
            sender: None,
            handle: None,
            writer,
        })
    }

    /// Returns a shared reference to the value vector so the caller can update
    /// it while the timed writer is running.
    pub fn get_value_access(&self) -> Arc<Mutex<Vector>> {
        self.value.clone()
    }

    /// Starts the background send loop.  Calling `start` on an already-running
    /// writer is a no-op.
    pub fn start(&mut self) {
        use std::thread::spawn;

        // If this is already running, we don't do anything
        if self.sender.is_some() {
            return;
        }

        // Make copies such that self isn't moved here
        let thread_value = self.value.clone();
        let thread_timeout = Duration::from_secs_f64(1.0 / self.frequency);
        let thread_writer = self.writer.sender.clone();

        // Create a channel to later terminate the thread
        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender);

        self.handle = Some(spawn(move || loop {
            let value = thread_value.lock().unwrap().clone();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring timestamp failed!")
                .as_secs_f64();
            let _ = thread_writer.send((value, timestamp));

            // Break if notified via channel or disconnected
            match receiver.recv_timeout(thread_timeout) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => (),
            }
        }));
    }

    /// Signals the background thread to stop and joins it.  Calling `stop` when
    /// not running is a no-op.
    pub fn stop(&mut self) {
        if self.sender.is_some() {
            if let Some(sender) = self.sender.take() {
                // The send might fail if the receiver is already gone, which is fine.
                let _ = sender.send(());
            }
            if let Some(handle) = self.handle.take() {
                handle.join().expect("Could not join with writer thread!");
            }
        }
    }
}

impl Drop for TimedIpcWriter {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Passes a probability vector straight through to the circuit leaf.
pub struct IpcProbabilityWriter {
    inner: IpcWriter,
}

impl IpcProbabilityWriter {
    /// Wraps `sender` in a probability writer.
    pub fn new(sender: Sender<(Vector, f64)>) -> Self {
        Self {
            inner: IpcWriter::new(sender).unwrap(),
        }
    }

    /// Sends `value` as-is with the given `timestamp`.
    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        self.inner.write(value, timestamp);
    }
}

/// Abramowitz & Stegun 7.1.26 — max error < 1.5e-7, no external deps.
/// LLVM can auto-vectorize the polynomial part across ndarray mapv loops.
#[inline]
fn erf(x: f64) -> f64 {
    let sign = if x >= 0.0 { 1.0 } else { -1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let p = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));

    sign * (1.0 - p * (-x * x).exp())
}

pub enum VectorDistribution {
    /// `params: [means, stds]`
    Normal { mean: Vector, std: Vector },
    /// `params: [log_means, log_stds]` — natural-log space mean and std
    LogNormal { log_mean: Vector, log_std: Vector },
    /// `params: [rates]` — rate λ, i.e. `Exp(λ)` with mean 1/λ
    Exponential { rate: Vector },
    /// `params: [lows, highs]`
    Uniform { low: Vector, high: Vector },
}

impl VectorDistribution {
    /// P(X ≤ threshold) evaluated element-wise across the parameter vectors.
    pub fn cdf(&self, threshold: f64) -> Vector {
        use std::f64::consts::SQRT_2;
        match self {
            VectorDistribution::Normal { mean, std } => {
                let values: Vec<f64> = mean
                    .iter()
                    .zip(std.iter())
                    .map(|(&m, &s)| 0.5 * (1.0 + erf((threshold - m) / (s * SQRT_2))))
                    .collect();
                ndarray::Array1::from(values).into_shared()
            }
            VectorDistribution::LogNormal { log_mean, log_std } => {
                if threshold <= 0.0 {
                    return Vector::from_elem(log_mean.len(), 0.0);
                }
                let log_t = threshold.ln();
                let values: Vec<f64> = log_mean
                    .iter()
                    .zip(log_std.iter())
                    .map(|(&m, &s)| 0.5 * (1.0 + erf((log_t - m) / (s * SQRT_2))))
                    .collect();
                ndarray::Array1::from(values).into_shared()
            }
            VectorDistribution::Exponential { rate } => {
                if threshold <= 0.0 {
                    return Vector::from_elem(rate.len(), 0.0);
                }
                rate.mapv(|r| 1.0 - (-r * threshold).exp()).into_shared()
            }
            VectorDistribution::Uniform { low, high } => {
                let values: Vec<f64> = low
                    .iter()
                    .zip(high.iter())
                    .map(|(&lo, &hi)| {
                        if threshold <= lo {
                            0.0
                        } else if threshold >= hi {
                            1.0
                        } else {
                            (threshold - lo) / (hi - lo)
                        }
                    })
                    .collect();
                ndarray::Array1::from(values).into_shared()
            }
        }
    }

    /// P(X > threshold) = 1 − CDF(threshold), element-wise.
    pub fn sf(&self, threshold: f64) -> Vector {
        let c = self.cdf(threshold);
        Vector::ones(c.len()) - &*c
    }
}

/// Fan-out density writer.  A single `write(&distribution, ts)` call dispatches
/// to every registered comparison channel, computing CDF or SF element-wise
/// across all value-space slots:
/// - `upper_tail = false` → CDF(threshold)  = P(X ≤ threshold)
/// - `upper_tail = true`  → SF(threshold)   = P(X > threshold)
pub struct IpcDensityWriter {
    // (threshold, upper_tail, sender)
    channels: Vec<ComparisonChannel>,
}

impl IpcDensityWriter {
    /// Creates a density writer with a single comparison channel.
    /// Convenient for direct use without the Resin compiler.
    pub fn new(sender: Sender<(Vector, f64)>, threshold: f64, upper_tail: bool) -> Self {
        Self {
            channels: vec![(threshold, upper_tail, sender)],
        }
    }

    /// Multi-comparison constructor used by the Resin compiler.
    pub fn from_channels(channels: Vec<ComparisonChannel>) -> Self {
        Self { channels }
    }

    /// Computes CDF or SF for each registered threshold and sends the results.
    pub fn write(&self, distribution: &VectorDistribution, timestamp: Option<f64>) {
        let ts = resolve_timestamp(timestamp);
        for (threshold, upper_tail, sender) in &self.channels {
            let probability = if *upper_tail {
                distribution.sf(*threshold)
            } else {
                distribution.cdf(*threshold)
            };
            let _ = sender.send((probability, ts));
        }
    }
}

/// Fan-out number writer.  Maps a single `f64` measurement to 0.0/1.0 for
/// every registered comparison channel:
/// - `upper_tail = false` → 1.0 when `value < threshold`
/// - `upper_tail = true`  → 1.0 when `value > threshold`
pub struct IpcNumberWriter {
    // (threshold, upper_tail, sender)
    channels: Vec<ComparisonChannel>,
}

impl IpcNumberWriter {
    /// Creates a number writer with a single comparison channel.
    pub fn new(sender: Sender<(Vector, f64)>, threshold: f64, upper_tail: bool) -> Self {
        Self {
            channels: vec![(threshold, upper_tail, sender)],
        }
    }

    /// Multi-comparison constructor used by the Resin compiler.
    pub fn from_channels(channels: Vec<ComparisonChannel>) -> Self {
        Self { channels }
    }

    /// Maps each element of `value` to `1.0` or `0.0` based on each registered
    /// threshold comparison and sends the results.
    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let ts = resolve_timestamp(timestamp);
        for (threshold, upper_tail, sender) in &self.channels {
            let probability = value
                .mapv(|v| {
                    if *upper_tail {
                        if v > *threshold {
                            1.0
                        } else {
                            0.0
                        }
                    } else if v < *threshold {
                        1.0
                    } else {
                        0.0
                    }
                })
                .into_shared();
            let _ = sender.send((probability, ts));
        }
    }
}

/// Interval writer for `Density` sources.  A single `write` computes the mass
/// of each interval induced by the source's thresholds:
///   u_0 = F(t_0),  u_k = F(t_k) - F(t_{k-1}),  u_K = 1 - F(t_{K-1}).
/// Emits one flat `[col_0, col_1, ..., col_K]` vector, matching the layout of
/// `IpcCategoricalReader`.
pub struct IpcDensityIntervalWriter {
    thresholds: Vec<f64>, // sorted ascending
    inner: IpcWriter,
}

impl IpcDensityIntervalWriter {
    pub fn new(sender: Sender<(Vector, f64)>, mut thresholds: Vec<f64>) -> Self {
        thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        thresholds.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        Self {
            thresholds,
            inner: IpcWriter::new(sender).unwrap(),
        }
    }

    pub fn n_intervals(&self) -> usize {
        self.thresholds.len() + 1
    }

    /// Computes each interval's mass for `distribution` and sends the flat
    /// `[col_0, ..., col_K]` vector.  The CDF is evaluated once per threshold
    /// (not once per interval) and consecutive evaluations are differenced.
    pub fn write(&self, distribution: &VectorDistribution, timestamp: Option<f64>) {
        let cdfs: Vec<Vector> = self
            .thresholds
            .iter()
            .map(|&t| distribution.cdf(t))
            .collect();
        let value_size = cdfs.first().map(|c| c.len()).unwrap_or(1);

        let mut flat: Vec<f64> = Vec::with_capacity(self.n_intervals() * value_size);
        for k in 0..self.n_intervals() {
            #[allow(clippy::needless_range_loop)] // indexes cdfs[k-1] and cdfs[k] by the same i
            for i in 0..value_size {
                let lo = if k == 0 { 0.0 } else { cdfs[k - 1][i] };
                let hi = if k == self.thresholds.len() {
                    1.0
                } else {
                    cdfs[k][i]
                };
                flat.push((hi - lo).max(0.0)); // guard against CDF round-off
            }
        }
        self.inner
            .write(ndarray::Array1::from(flat).into_shared(), timestamp);
    }
}

/// Interval writer for `Number` sources.  One-hot on the interval containing
/// the observed value: u_k = 1 iff t_{k-1} < v <= t_k.
pub struct IpcNumberIntervalWriter {
    thresholds: Vec<f64>,
    inner: IpcWriter,
}

impl IpcNumberIntervalWriter {
    pub fn new(sender: Sender<(Vector, f64)>, mut thresholds: Vec<f64>) -> Self {
        thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        thresholds.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        Self {
            thresholds,
            inner: IpcWriter::new(sender).unwrap(),
        }
    }

    pub fn n_intervals(&self) -> usize {
        self.thresholds.len() + 1
    }

    /// One-hots `value` element-wise into its containing interval and sends
    /// the flat `[col_0, ..., col_K]` vector.
    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let value_size = value.len();
        let mut flat = vec![0.0; self.n_intervals() * value_size];
        for (i, &v) in value.iter().enumerate() {
            let k = self
                .thresholds
                .iter()
                .position(|&t| v <= t)
                .unwrap_or(self.thresholds.len());
            flat[k * value_size + i] = 1.0;
        }
        self.inner
            .write(ndarray::Array1::from(flat).into_shared(), timestamp);
    }
}

/// Maps a boolean to a probability: `true` → 1.0, `false` → 0.0.
pub struct IpcBooleanWriter {
    inner: IpcWriter,
}

impl IpcBooleanWriter {
    /// Wraps `sender` in a boolean writer.
    pub fn new(sender: Sender<(Vector, f64)>) -> Self {
        Self {
            inner: IpcWriter::new(sender).unwrap(),
        }
    }

    /// Converts `value` to `1.0` (`true`) or `0.0` (`false`) and sends it.
    pub fn write(&self, value: bool, timestamp: Option<f64>) {
        self.inner.write(
            Vector::from_elem(1, if value { 1.0 } else { 0.0 }),
            timestamp,
        );
    }
}

/// Reads a flat probability matrix from a channel and updates one leaf per category.
///
/// The incoming `Vector` has layout `[col₀, col₁, …, colₙ₋₁]` where each `colₖ`
/// is a contiguous block of `value_size` floats — the probability of category `k`
/// across all batch slots.  Leaf `k` is updated to `colₖ`.
pub struct IpcCategoricalReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>,
}

impl IpcCategoricalReader {
    pub fn new<S: Semiring>(
        shared_rc: Arc<Mutex<ReactiveCircuit<S>>>,
        category_indices: Vec<u32>,
        channel: &str,
        receiver: mpsc::Receiver<(Vector, f64)>,
    ) -> Self {
        let handle = std::thread::spawn(move || {
            while let Ok(mut latest) = receiver.recv() {
                // Collapse any backlog that built up while we were busy —
                // leaves represent current state, so only the most recent
                // value matters and applying stale ones is wasted work.
                while let Ok(next) = receiver.try_recv() {
                    latest = next;
                }
                let (probs, timestamp) = latest;
                // Stride is recomputed from the message itself rather than
                // captured at construction time: `Resin::compile` can override
                // `value_size` (e.g. `ProbGradient::auto_value_size`) after this
                // reader is created, which would otherwise leave it slicing the
                // wrong columns.
                if category_indices.is_empty() {
                    continue;
                }
                let value_size = probs.len() / category_indices.len();
                let mut circuit = shared_rc.lock().unwrap();
                for (k, &leaf_idx) in category_indices.iter().enumerate() {
                    let start = k * value_size;
                    let end = start + value_size;
                    if end <= probs.len() {
                        let col = probs.slice(ndarray::s![start..end]).to_shared();
                        update(&mut circuit, leaf_idx, col, timestamp);
                    }
                }
            }
        });
        Self {
            topic: channel.to_owned(),
            _handle: Arc::new(handle),
        }
    }
}

/// Sends a flat probability matrix `[col₀, col₁, …, colₙ₋₁]` to a categorical
/// channel.  Each `colₖ` is a contiguous block of `value_size` values.
pub struct IpcCategoricalWriter {
    inner: IpcWriter,
    n_categories: usize,
    value_size: usize,
}

impl IpcCategoricalWriter {
    pub fn new(sender: Sender<(Vector, f64)>, n_categories: usize, value_size: usize) -> Self {
        Self {
            inner: IpcWriter::new(sender).unwrap(),
            n_categories,
            value_size,
        }
    }

    pub fn n_categories(&self) -> usize {
        self.n_categories
    }
    pub fn value_size(&self) -> usize {
        self.value_size
    }

    /// Write a flat probability matrix.  `probabilities` must have length
    /// `n_categories * value_size`, laid out as `n_categories` consecutive
    /// columns of `value_size` entries each.
    pub fn write(&self, probabilities: Vector, timestamp: Option<f64>) {
        debug_assert_eq!(
            probabilities.len(),
            self.n_categories * self.value_size,
            "categorical write: expected {} values, got {}",
            self.n_categories * self.value_size,
            probabilities.len()
        );
        self.inner.write(probabilities, timestamp);
    }
}

/// Groups all typed writers so callers can handle them in a single `match`.
pub enum TypedWriter {
    Probability(IpcProbabilityWriter),
    Density(IpcDensityIntervalWriter),
    Number(IpcNumberIntervalWriter),
    Boolean(IpcBooleanWriter),
    Categorical(IpcCategoricalWriter),
}

/// Returns `timestamp` if `Some`, otherwise returns the current Unix time in seconds.
fn resolve_timestamp(timestamp: Option<f64>) -> f64 {
    timestamp.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Acquiring UNIX timestamp failed!")
            .as_secs_f64()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::semiring::LogProb;
    use ndarray::array;
    use std::thread::sleep;

    type TestRC = ReactiveCircuit<LogProb>;

    // -----------------------------------------------------------------------
    // Typed writer tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_probability_writer() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        let writer = IpcProbabilityWriter::new(tx);
        writer.write(array![0.7].into(), None);
        let (value, _) = rx.try_recv().unwrap();
        assert!((value[0] - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_boolean_writer() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        let writer = IpcBooleanWriter::new(tx);

        writer.write(true, None);
        let (value, _) = rx.try_recv().unwrap();
        assert_eq!(value[0], 1.0);

        writer.write(false, None);
        let (value, _) = rx.try_recv().unwrap();
        assert_eq!(value[0], 0.0);
    }

    #[test]
    fn test_number_writer_less_than() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        // upper_tail = false → 1.0 when value < threshold
        let writer = IpcNumberWriter::new(tx, 10.0, false);

        writer.write(array![5.0].into(), None);
        let (v, _) = rx.try_recv().unwrap();
        assert_eq!(v[0], 1.0);

        writer.write(array![15.0].into(), None);
        let (v, _) = rx.try_recv().unwrap();
        assert_eq!(v[0], 0.0);
    }

    #[test]
    fn test_number_writer_greater_than() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        // upper_tail = true → 1.0 when value > threshold
        let writer = IpcNumberWriter::new(tx, 10.0, true);

        writer.write(array![15.0].into(), None);
        let (v, _) = rx.try_recv().unwrap();
        assert_eq!(v[0], 1.0);

        writer.write(array![5.0].into(), None);
        let (v, _) = rx.try_recv().unwrap();
        assert_eq!(v[0], 0.0);
    }

    #[test]
    fn test_number_writer_fan_out() {
        let (tx_lt, rx_lt) = mpsc::channel::<(Vector, f64)>();
        let (tx_gt, rx_gt) = mpsc::channel::<(Vector, f64)>();
        // Fan-out: one channel for < 10, one for > 50
        let writer =
            IpcNumberWriter::from_channels(vec![(10.0, false, tx_lt), (50.0, true, tx_gt)]);

        writer.write(array![5.0].into(), None); // < 10 → 1.0 | > 50 → 0.0
        assert_eq!(rx_lt.try_recv().unwrap().0[0], 1.0);
        assert_eq!(rx_gt.try_recv().unwrap().0[0], 0.0);

        writer.write(array![60.0].into(), None); // < 10 → 0.0 | > 50 → 1.0
        assert_eq!(rx_lt.try_recv().unwrap().0[0], 0.0);
        assert_eq!(rx_gt.try_recv().unwrap().0[0], 1.0);
    }

    #[test]
    fn test_density_writer_fan_out() {
        let (tx_lt, rx_lt) = mpsc::channel::<(Vector, f64)>();
        let (tx_gt, rx_gt) = mpsc::channel::<(Vector, f64)>();
        // Fan-out: P(X < 20) and P(X > 55) for Normal(25, 5)
        let writer =
            IpcDensityWriter::from_channels(vec![(20.0, false, tx_lt), (55.0, true, tx_gt)]);

        let dist = VectorDistribution::Normal {
            mean: Vector::from_elem(1, 25.0),
            std: Vector::from_elem(1, 5.0),
        };
        writer.write(&dist, None);

        let p_lt = rx_lt.try_recv().unwrap().0[0];
        let p_gt = rx_gt.try_recv().unwrap().0[0];

        // P(X < 20) for Normal(25, 5): z = (20-25)/5 = -1 → CDF ≈ 0.159
        assert!((p_lt - 0.159).abs() < 0.001, "p_lt = {}", p_lt);
        // P(X > 55) for Normal(25, 5): z = (55-25)/5 = 6 → SF ≈ 0
        assert!(p_gt < 1e-6, "p_gt = {}", p_gt);
    }

    #[test]
    fn test_density_interval_writer_single_threshold() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        let writer = IpcDensityIntervalWriter::new(tx, vec![20.0]);
        assert_eq!(writer.n_intervals(), 2);

        let dist = VectorDistribution::Normal {
            mean: Vector::from_elem(1, 25.0),
            std: Vector::from_elem(1, 5.0),
        };
        writer.write(&dist, None);

        let (flat, _) = rx.try_recv().unwrap();
        assert_eq!(flat.len(), 2);
        // u_0 = F(20), u_1 = 1 - F(20); z = (20-25)/5 = -1 -> F(20) ≈ 0.159
        assert!((flat[0] - 0.159).abs() < 0.001, "u_0 = {}", flat[0]);
        assert!(
            (flat[0] + flat[1] - 1.0).abs() < 1e-9,
            "masses must sum to 1"
        );
    }

    #[test]
    fn test_density_interval_writer_two_thresholds_sum_to_one() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        let writer = IpcDensityIntervalWriter::new(tx, vec![20.0, 30.0]);
        assert_eq!(writer.n_intervals(), 3);

        let dist = VectorDistribution::Normal {
            mean: Vector::from_elem(1, 25.0),
            std: Vector::from_elem(1, 1.0),
        };
        writer.write(&dist, None);

        let (flat, _) = rx.try_recv().unwrap();
        assert_eq!(flat.len(), 3);
        let total: f64 = flat.iter().sum();
        assert!((total - 1.0).abs() < 1e-9, "masses must sum to 1: {flat:?}");
        assert!(
            flat.iter().all(|&m| m >= 0.0),
            "no negative masses: {flat:?}"
        );
    }

    #[test]
    fn test_number_interval_writer_one_hot() {
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        // Thresholds 20, 30 -> I_0=(-inf,20], I_1=(20,30], I_2=(30,inf)
        let writer = IpcNumberIntervalWriter::new(tx, vec![20.0, 30.0]);

        writer.write(array![25.0].into(), None);
        let (flat, _) = rx.try_recv().unwrap();
        assert_eq!(&flat.to_vec(), &[0.0, 1.0, 0.0]);

        writer.write(array![5.0].into(), None);
        let (flat, _) = rx.try_recv().unwrap();
        assert_eq!(&flat.to_vec(), &[1.0, 0.0, 0.0]);

        writer.write(array![100.0].into(), None);
        let (flat, _) = rx.try_recv().unwrap();
        assert_eq!(&flat.to_vec(), &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn test_number_interval_writer_boundary_is_inclusive_left_side() {
        // Half-open right convention: v == t falls in the interval ending at t,
        // unlike the old strict IpcNumberWriter where v == t satisfied neither
        // `< t` nor `> t`.
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        let writer = IpcNumberIntervalWriter::new(tx, vec![10.0]);

        writer.write(array![10.0].into(), None);
        let (flat, _) = rx.try_recv().unwrap();
        assert_eq!(&flat.to_vec(), &[1.0, 0.0], "v == t must land in I_0 (< t)");
    }

    // -----------------------------------------------------------------------
    // Vectorized distribution CDF tests
    // -----------------------------------------------------------------------

    /// Reference: Normal CDF computed from the standard z-table.
    fn normal_cdf_ref(x: f64, mean: f64, std: f64) -> f64 {
        let z = (x - mean) / (std * std::f64::consts::SQRT_2);
        0.5 * (1.0 + erf(z))
    }

    #[test]
    fn test_vector_distribution_normal_many_values() {
        const N: usize = 10_000;
        // N distributions with means spread from -5 to 5 and stds from 0.5 to 2.0
        let means: Vec<f64> = (0..N)
            .map(|i| -5.0 + 10.0 * i as f64 / (N - 1) as f64)
            .collect();
        let stds: Vec<f64> = (0..N)
            .map(|i| 0.5 + 1.5 * i as f64 / (N - 1) as f64)
            .collect();
        let threshold = 0.0;

        let dist = VectorDistribution::Normal {
            mean: Vector::from(means.clone()),
            std: Vector::from(stds.clone()),
        };

        let result = dist.cdf(threshold);
        assert_eq!(result.len(), N);

        for (i, (&p, (&m, &s))) in result.iter().zip(means.iter().zip(stds.iter())).enumerate() {
            let expected = normal_cdf_ref(threshold, m, s);
            assert!(
                (p - expected).abs() < 1e-6,
                "element {i}: got {p}, expected {expected}"
            );
        }

        // sf = 1 - cdf
        let sf = dist.sf(threshold);
        for (p, s) in result.iter().zip(sf.iter()) {
            assert!((p + s - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn test_vector_distribution_lognormal_many_values() {
        const N: usize = 1_000;
        // Log-means and log-stds for LN distributions; threshold is positive
        let log_means: Vec<f64> = (0..N).map(|i| i as f64 / N as f64).collect();
        let log_stds: Vec<f64> = vec![0.5; N];
        let threshold = 1.5_f64;

        let dist = VectorDistribution::LogNormal {
            log_mean: Vector::from(log_means.clone()),
            log_std: Vector::from(log_stds.clone()),
        };
        let result = dist.cdf(threshold);
        assert_eq!(result.len(), N);

        for (i, (&p, (&m, &s))) in result
            .iter()
            .zip(log_means.iter().zip(log_stds.iter()))
            .enumerate()
        {
            let expected = normal_cdf_ref(threshold.ln(), m, s);
            assert!(
                (p - expected).abs() < 1e-6,
                "element {i}: got {p}, expected {expected}"
            );
        }
    }

    #[test]
    fn test_vector_distribution_exponential_many_values() {
        const N: usize = 1_000;
        let rates: Vec<f64> = (1..=N).map(|i| i as f64 / 100.0).collect();
        let threshold = 2.0_f64;

        let dist = VectorDistribution::Exponential {
            rate: Vector::from(rates.clone()),
        };
        let result = dist.cdf(threshold);
        assert_eq!(result.len(), N);

        for (i, (&p, &r)) in result.iter().zip(rates.iter()).enumerate() {
            let expected = 1.0 - (-r * threshold).exp();
            assert!(
                (p - expected).abs() < 1e-12,
                "element {i}: got {p}, expected {expected}"
            );
        }
    }

    #[test]
    fn test_vector_distribution_uniform_many_values() {
        const N: usize = 1_000;
        let lows: Vec<f64> = vec![0.0; N];
        let highs: Vec<f64> = (1..=N).map(|i| i as f64).collect(); // widths 1..N
        let threshold = 0.5_f64;

        let dist = VectorDistribution::Uniform {
            low: Vector::from(lows.clone()),
            high: Vector::from(highs.clone()),
        };
        let result = dist.cdf(threshold);
        assert_eq!(result.len(), N);

        for (i, (&p, (&lo, &hi))) in result.iter().zip(lows.iter().zip(highs.iter())).enumerate() {
            let expected = if threshold <= lo {
                0.0
            } else if threshold >= hi {
                1.0
            } else {
                (threshold - lo) / (hi - lo)
            };
            assert!(
                (p - expected).abs() < 1e-12,
                "element {i}: got {p}, expected {expected}"
            );
        }
    }

    #[test]
    fn test_density_writer_many_particles() {
        // Simulates a particle filter with 10_000 particles, each with its
        // own Normal distribution parameters.
        const N: usize = 10_000;
        let (tx, rx) = mpsc::channel::<(Vector, f64)>();
        let writer = IpcDensityWriter::new(tx, 0.0, false); // CDF at threshold=0

        let means: Vec<f64> = (0..N)
            .map(|i| -5.0 + 10.0 * i as f64 / (N - 1) as f64)
            .collect();
        let stds: Vec<f64> = vec![1.0; N];
        let dist = VectorDistribution::Normal {
            mean: Vector::from(means.clone()),
            std: Vector::from(stds),
        };

        writer.write(&dist, None);

        let (result, _) = rx.try_recv().unwrap();
        assert_eq!(result.len(), N);

        // For mean < 0 (lower half), CDF(0) > 0.5
        let lower_half_mean = result[0]; // mean = -5.0
        assert!(
            lower_half_mean > 0.9,
            "CDF(0) for N(-5,1) should be near 1: {lower_half_mean}"
        );

        // For mean = 0 (midpoint), CDF(0) ≈ 0.5
        let mid = result[N / 2];
        assert!(
            (mid - 0.5).abs() < 0.01,
            "CDF(0) for N(0,1) should be ~0.5: {mid}"
        );

        // For mean > 0 (upper half), CDF(0) < 0.5
        let upper_half_mean = result[N - 1]; // mean = 5.0
        assert!(
            upper_half_mean < 0.1,
            "CDF(0) for N(5,1) should be near 0: {upper_half_mean}"
        );
    }

    #[test]
    fn test_ipc_read_write() -> Result<(), Box<dyn std::error::Error>> {
        let reactive_circuit = Arc::new(Mutex::new(TestRC::new(1)));
        reactive_circuit
            .lock()
            .unwrap()
            .leafs
            .push(crate::circuit::leaf::Leaf::new(
                array![0.0].into(),
                0.0,
                "test_leaf",
                0,
            ));
        let (tx, rx) = mpsc::channel();
        let _reader = IpcReader::new(reactive_circuit.clone(), 0, "test_channel", false, rx)?;
        let writer = IpcWriter::new(tx)?;

        assert_eq!(
            reactive_circuit.lock().unwrap().leafs[0].get_value(),
            array![0.0]
        );

        writer.write(array![0.5].into(), None);

        // Give the reader thread time to process
        sleep(Duration::from_millis(20));

        // Check updated value
        assert_eq!(
            reactive_circuit.lock().unwrap().leafs[0].get_value(),
            array![0.5]
        );

        // Test inversion
        let (tx_invert, rx_invert) = mpsc::channel();
        let _reader_invert = IpcReader::new(
            reactive_circuit.clone(),
            0,
            "test_channel_invert",
            true,
            rx_invert,
        )?;
        let writer_invert = IpcWriter::new(tx_invert)?;

        writer_invert.write(array![0.8].into(), None);
        sleep(Duration::from_millis(20));

        // The value should be 1.0 - 0.8
        assert!(
            (reactive_circuit.lock().unwrap().leafs[0].get_value() - array![0.2])
                .sum()
                .abs()
                < 1e-9,
            "Inversion failed"
        );

        Ok(())
    }

    #[test]
    fn test_timed_ipc_writer() -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        let mut timed_writer = TimedIpcWriter::new(100.0, tx, array![0.0].into())?; // 100 Hz

        // Get access to the value
        let value_access = timed_writer.get_value_access();
        *value_access.lock().unwrap() = array![0.25].into();

        // Start the writer
        timed_writer.start();

        // Wait for a couple of cycles
        sleep(Duration::from_millis(30));

        // Stop the writer
        timed_writer.stop();

        // Change the value again
        *value_access.lock().unwrap() = array![0.75].into();

        // Wait again
        sleep(Duration::from_millis(30));

        // Collect received values
        let mut received_values = vec![];
        while let Ok((val, _)) = rx.try_recv() {
            received_values.push(val);
        }

        // We should have received some values (likely 2 or 3)
        assert!(!received_values.is_empty());

        // All received values should be 0.25, as the writer was stopped before 0.75 was set
        for val in &received_values {
            assert_eq!(*val, array![0.25]);
        }

        // Check that no 0.75 values were sent
        assert!(!received_values.contains(&array![0.75].into()));

        // Test drop behavior
        let (tx2, rx2) = mpsc::channel();
        {
            let mut timed_writer2 = TimedIpcWriter::new(100.0, tx2, array![0.0].into())?;
            timed_writer2.start();
        } // timed_writer2 is dropped here, stopping the thread

        // Drain the channel for possible remaining data
        while rx2.try_recv().is_ok() {
            // Keep draining
        }

        // Now that the channel is empty, the next call should show it's disconnected
        assert_eq!(
            rx2.try_recv(),
            Err(mpsc::TryRecvError::Disconnected),
            "Channel should be disconnected after writer is dropped"
        );

        Ok(())
    }
}
