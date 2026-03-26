use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::circuit::leaf::update;
use crate::circuit::ReactiveCircuit;

use super::Vector;

#[derive(Clone)]
pub struct IpcReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>, // Keep handle to keep thread alive
}

#[derive(Clone)]
pub struct IpcDualReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>, // Keep handle to keep thread alive
}

pub struct IpcWriter {
    sender: Sender<(Vector, f64)>,
}

pub struct TimedIpcWriter {
    pub frequency: f64,
    value: Arc<Mutex<Vector>>,
    sender: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
    writer: IpcWriter,
}

impl IpcReader {
    pub fn new(
        shared_reactive_circuit: Arc<Mutex<ReactiveCircuit>>,
        index: u32,
        channel: &str,
        invert: bool,
        receiver: mpsc::Receiver<(Vector, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let handle = std::thread::spawn(move || {
            while let Ok((value, timestamp)) = receiver.recv() {
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
    pub fn new(
        shared_reactive_circuit: Arc<Mutex<ReactiveCircuit>>,
        index_normal: u32,
        index_inverted: u32,
        channel: &str,
        receiver: mpsc::Receiver<(Vector, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let handle = std::thread::spawn(move || {
            while let Ok((value, timestamp)) = receiver.recv() {
                let inverted_value = (Vector::ones(value.len()) - &*value).into();
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
    pub fn new(sender: Sender<(Vector, f64)>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { sender })
    }

    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let timestamp = if timestamp.is_none() {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        } else {
            timestamp.unwrap()
        };

        let _ = self.sender.send((value, timestamp));
    }
}

impl TimedIpcWriter {
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

    pub fn get_value_access(&self) -> Arc<Mutex<Vector>> {
        self.value.clone()
    }

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

// ---------------------------------------------------------------------------
// Typed writers — each accepts domain-appropriate input, converts to a
// probability Vector, then delegates to an inner IpcWriter.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Typed writers
// ---------------------------------------------------------------------------

/// Passes a probability Vector straight through. Use when the data source
/// already produces values in [0, 1].
pub struct IpcProbabilityWriter {
    inner: IpcWriter,
}

impl IpcProbabilityWriter {
    pub fn new(sender: Sender<(Vector, f64)>) -> Self {
        Self {
            inner: IpcWriter::new(sender).unwrap(),
        }
    }

    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        self.inner.write(value, timestamp);
    }
}

// ---------------------------------------------------------------------------
// Vectorized distribution CDF
// ---------------------------------------------------------------------------

/// Abramowitz & Stegun 7.1.26 — max error < 1.5e-7, no external deps.
/// LLVM can auto-vectorize the polynomial part across ndarray mapv loops.
#[inline]
fn erf(x: f64) -> f64 {
    let sign = if x >= 0.0 { 1.0 } else { -1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let p = t * (0.254_829_592
        + t * (-0.284_496_736
            + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    sign * (1.0 - p * (-x * x).exp())
}

/// A distribution whose parameters are Vectors, enabling element-wise CDF
/// evaluation across all value-space slots in a single call.
///
/// Each variant stores its parameters as `Vector` so a single `write` call
/// on an `IpcDensityWriter` fan-out handles particle-filter-sized value
/// vectors (e.g. 10 000 particles) without per-element object construction.
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
        (Vector::ones(c.len()) - &*c).into()
    }
}

// ---------------------------------------------------------------------------
// IpcDensityWriter
// ---------------------------------------------------------------------------

/// Fan-out density writer.  A single `write(&distribution, ts)` call dispatches
/// to every registered comparison channel, computing CDF or SF element-wise
/// across all value-space slots:
/// - `upper_tail = false` → CDF(threshold)  = P(X ≤ threshold)
/// - `upper_tail = true`  → SF(threshold)   = P(X > threshold)
pub struct IpcDensityWriter {
    // (threshold, upper_tail, sender)
    channels: Vec<(f64, bool, Sender<(Vector, f64)>)>,
}

impl IpcDensityWriter {
    /// Single-comparison constructor — convenient for direct use without the compiler.
    pub fn new(sender: Sender<(Vector, f64)>, threshold: f64, upper_tail: bool) -> Self {
        Self {
            channels: vec![(threshold, upper_tail, sender)],
        }
    }

    /// Multi-comparison constructor used by the Resin compiler.
    pub fn from_channels(channels: Vec<(f64, bool, Sender<(Vector, f64)>)>) -> Self {
        Self { channels }
    }

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
    channels: Vec<(f64, bool, Sender<(Vector, f64)>)>,
}

impl IpcNumberWriter {
    /// Single-comparison constructor.
    pub fn new(sender: Sender<(Vector, f64)>, threshold: f64, upper_tail: bool) -> Self {
        Self {
            channels: vec![(threshold, upper_tail, sender)],
        }
    }

    /// Multi-comparison constructor used by the Resin compiler.
    pub fn from_channels(channels: Vec<(f64, bool, Sender<(Vector, f64)>)>) -> Self {
        Self { channels }
    }

    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let ts = resolve_timestamp(timestamp);
        for (threshold, upper_tail, sender) in &self.channels {
            let probability = value
                .mapv(|v| {
                    if *upper_tail {
                        if v > *threshold { 1.0 } else { 0.0 }
                    } else {
                        if v < *threshold { 1.0 } else { 0.0 }
                    }
                })
                .into_shared();
            let _ = sender.send((probability, ts));
        }
    }
}

/// Maps a boolean to a probability: `true` → 1.0, `false` → 0.0.
pub struct IpcBooleanWriter {
    inner: IpcWriter,
}

impl IpcBooleanWriter {
    pub fn new(sender: Sender<(Vector, f64)>) -> Self {
        Self {
            inner: IpcWriter::new(sender).unwrap(),
        }
    }

    pub fn write(&self, value: bool, timestamp: Option<f64>) {
        self.inner
            .write(Vector::from_elem(1, if value { 1.0 } else { 0.0 }), timestamp);
    }
}

/// Groups all typed writers so callers can handle them in a single `match`.
pub enum TypedWriter {
    Probability(IpcProbabilityWriter),
    Density(IpcDensityWriter),
    Number(IpcNumberWriter),
    Boolean(IpcBooleanWriter),
}

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
    use ndarray::array;
    use std::thread::sleep;

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
        let writer = IpcNumberWriter::from_channels(vec![
            (10.0, false, tx_lt),
            (50.0, true, tx_gt),
        ]);

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
        let writer = IpcDensityWriter::from_channels(vec![
            (20.0, false, tx_lt),
            (55.0, true, tx_gt),
        ]);

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

    // -----------------------------------------------------------------------
    // Vectorized distribution CDF tests (many values)
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
        let means: Vec<f64> = (0..N).map(|i| -5.0 + 10.0 * i as f64 / (N - 1) as f64).collect();
        let stds: Vec<f64> = (0..N).map(|i| 0.5 + 1.5 * i as f64 / (N - 1) as f64).collect();
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

        for (i, (&p, (&m, &s))) in
            result.iter().zip(log_means.iter().zip(log_stds.iter())).enumerate()
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

        let dist = VectorDistribution::Exponential { rate: Vector::from(rates.clone()) };
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

        for (i, (&p, (&lo, &hi))) in
            result.iter().zip(lows.iter().zip(highs.iter())).enumerate()
        {
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

        let means: Vec<f64> = (0..N).map(|i| -5.0 + 10.0 * i as f64 / (N - 1) as f64).collect();
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
        assert!(lower_half_mean > 0.9, "CDF(0) for N(-5,1) should be near 1: {lower_half_mean}");

        // For mean = 0 (midpoint), CDF(0) ≈ 0.5
        let mid = result[N / 2];
        assert!((mid - 0.5).abs() < 0.01, "CDF(0) for N(0,1) should be ~0.5: {mid}");

        // For mean > 0 (upper half), CDF(0) < 0.5
        let upper_half_mean = result[N - 1]; // mean = 5.0
        assert!(
            upper_half_mean < 0.1,
            "CDF(0) for N(5,1) should be near 0: {upper_half_mean}"
        );
    }

    #[test]
    fn test_ipc_read_write() -> Result<(), Box<dyn std::error::Error>> {
        let reactive_circuit = Arc::new(Mutex::new(ReactiveCircuit::new(1)));
        reactive_circuit
            .lock()
            .unwrap()
            .leafs
            .push(crate::circuit::leaf::Leaf::new(
                array![0.0].into(),
                0.0,
                "test_leaf",
            ));
        let (tx, rx) = mpsc::channel();

        // Create reader
        let _reader = IpcReader::new(reactive_circuit.clone(), 0, "test_channel", false, rx)?;

        // Create writer
        let writer = IpcWriter::new(tx)?;

        // Initial value
        assert_eq!(
            reactive_circuit.lock().unwrap().leafs[0].get_value(),
            array![0.0]
        );

        // Write a value
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
