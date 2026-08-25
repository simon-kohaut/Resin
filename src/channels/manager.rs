use std::{
    collections::HashMap,
    sync::mpsc,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::ipc::{
    IpcBooleanWriter, IpcCategoricalReader, IpcCategoricalWriter, IpcDensityIntervalWriter,
    IpcDensityWriter, IpcDualReader, IpcNumberIntervalWriter, IpcNumberWriter,
    IpcProbabilityWriter, IpcReader, IpcWriter, TimedIpcWriter,
};
use super::Vector;
use crate::circuit::{
    leaf::Leaf,
    reactive::ReactiveCircuit,
    semiring::{LogProb, Semiring},
};

/// Manages the state of leaves and the IPC channels for updating them.
///
/// `S` is the semiring used by the underlying `ReactiveCircuit`; the default
/// is `LogProb` so existing code compiles without annotation.
pub struct Manager<S: Semiring = LogProb> {
    pub reactive_circuit: Arc<Mutex<ReactiveCircuit<S>>>,
    readers: Vec<IpcReader>,
    dual_readers: Vec<IpcDualReader>,
    categorical_readers: Vec<IpcCategoricalReader>,
    writers: Vec<TimedIpcWriter>,
    senders: HashMap<String, mpsc::Sender<(Vector, f64)>>,
}

impl<S: Semiring> Default for Manager<S> {
    fn default() -> Self {
        Self::new(1)
    }
}

impl<S: Semiring> Manager<S> {
    /// Creates a new `Manager` with a fresh `ReactiveCircuit<S>` of the given
    /// `value_size` (number of parallel value slots, e.g. particles).
    pub fn new(value_size: usize) -> Self {
        Self {
            reactive_circuit: Arc::new(Mutex::new(ReactiveCircuit::<S>::new(value_size))),
            readers: vec![],
            dual_readers: vec![],
            categorical_readers: vec![],
            writers: vec![],
            senders: HashMap::new(),
        }
    }

    /// Creates a new `Leaf`.
    ///
    /// # Returns
    /// The index of the newly created leaf as a `u16`.
    pub fn create_leaf(&mut self, name: &str, value: Vector, frequency: f64) -> u32 {
        let mut rc = self.reactive_circuit.lock().unwrap();
        let leaf_index = rc.leafs.len();
        // This should never grow beyong u16.MAX since we use that range for indexing
        assert!(leaf_index + 1 < u32::MAX as usize);
        rc.leafs.push(Leaf::new(value, frequency, name, leaf_index));
        leaf_index as u32
    }

    /// Clears all dependency indices from all leaves and clears the reactive queue.
    pub fn clear_dependencies(&mut self) {
        for leaf in self.reactive_circuit.lock().unwrap().leafs.iter_mut() {
            leaf.clear_dependencies();
        }

        self.reactive_circuit.lock().unwrap().queue.clear();
    }

    /// Creates a reader for a given channel that updates a leaf.
    ///
    /// # Arguments
    /// * `receiver_idx` - The index of the leaf to be updated by this reader.
    /// * `channel` - The name of the IPC channel.
    /// * `invert` - If true, the received value will be inverted (1.0 - value).
    pub fn read(
        &mut self,
        receiver_idx: u32,
        channel: &str,
        invert: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        self.senders.insert(channel.to_string(), tx);
        let reader = IpcReader::new(
            self.reactive_circuit.clone(),
            receiver_idx,
            channel,
            invert,
            rx,
        )?;

        self.readers.push(reader);
        Ok(())
    }

    /// Creates a dual reader for a given channel that updates two leaves, one with the
    /// original value and one with an inverted value.
    ///
    /// # Arguments
    /// * `receiver_idx_normal` - The index of the leaf to be updated by this reader.
    /// * `receiver_idx_inverted` - The index of the leaf to be updated with an inverted value.
    /// * `channel` - The name of the IPC channel.
    pub fn read_dual(
        &mut self,
        index_normal: u32,
        index_inverted: u32,
        channel: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        self.senders.insert(channel.to_string(), tx);
        let reader = IpcDualReader::new(
            self.reactive_circuit.clone(),
            index_normal,
            index_inverted,
            channel,
            rx,
        )?;
        self.dual_readers.push(reader);
        Ok(())
    }

    /// Creates a categorical reader: reads a flat `[col₀, col₁, …]` vector and
    /// updates each category leaf to its column slice.
    pub fn read_categorical(
        &mut self,
        category_indices: Vec<u32>,
        channel: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        self.senders.insert(channel.to_string(), tx);
        let reader =
            IpcCategoricalReader::new(self.reactive_circuit.clone(), category_indices, channel, rx);
        self.categorical_readers.push(reader);
        Ok(())
    }

    /// Creates a categorical writer for `channel`.
    pub fn make_categorical_writer(
        &mut self,
        channel: &str,
        n_categories: usize,
    ) -> Result<IpcCategoricalWriter, Box<dyn std::error::Error>> {
        let value_size = self.reactive_circuit.lock().unwrap().value_size;
        Ok(IpcCategoricalWriter::new(
            self.get_or_create_sender(channel),
            n_categories,
            value_size,
        ))
    }

    /// Returns a cloned sender for `channel`, creating a dangling one if none exists yet.
    fn get_or_create_sender(&mut self, channel: &str) -> mpsc::Sender<(Vector, f64)> {
        if let Some(sender) = self.senders.get(channel) {
            sender.clone()
        } else {
            let (tx, _rx) = mpsc::channel();
            self.senders.insert(channel.to_string(), tx);
            self.senders.get(channel).unwrap().clone()
        }
    }

    /// Creates a writer for a given channel.
    pub fn make_writer(&mut self, channel: &str) -> Result<IpcWriter, Box<dyn std::error::Error>> {
        let sender = self.get_or_create_sender(channel);
        IpcWriter::new(sender)
    }

    /// Creates a typed probability writer that passes vectors straight through.
    pub fn make_probability_writer(
        &mut self,
        channel: &str,
    ) -> Result<IpcProbabilityWriter, Box<dyn std::error::Error>> {
        Ok(IpcProbabilityWriter::new(
            self.get_or_create_sender(channel),
        ))
    }

    /// Creates a single-comparison density writer for direct (non-compiler) use.
    /// `upper_tail = false` → CDF (P(X < threshold)); `true` → SF (P(X > threshold)).
    pub fn make_density_writer(
        &mut self,
        channel: &str,
        threshold: f64,
        upper_tail: bool,
    ) -> Result<IpcDensityWriter, Box<dyn std::error::Error>> {
        Ok(IpcDensityWriter::new(
            self.get_or_create_sender(channel),
            threshold,
            upper_tail,
        ))
    }

    /// Creates a single-comparison number writer for direct (non-compiler) use.
    /// `upper_tail = false` → 1.0 when value < threshold; `true` → 1.0 when value > threshold.
    pub fn make_number_writer(
        &mut self,
        channel: &str,
        threshold: f64,
        upper_tail: bool,
    ) -> Result<IpcNumberWriter, Box<dyn std::error::Error>> {
        Ok(IpcNumberWriter::new(
            self.get_or_create_sender(channel),
            threshold,
            upper_tail,
        ))
    }

    /// Creates an interval writer for a `Density` source.  `thresholds` are the
    /// comparison thresholds registered for that source; the writer emits one
    /// mass per induced interval on the source's own channel, in the flat
    /// layout expected by `IpcCategoricalReader`.
    pub fn make_density_interval_writer(
        &mut self,
        channel: &str,
        thresholds: Vec<f64>,
    ) -> Result<IpcDensityIntervalWriter, Box<dyn std::error::Error>> {
        Ok(IpcDensityIntervalWriter::new(
            self.get_or_create_sender(channel),
            thresholds,
        ))
    }

    /// Creates an interval writer for a `Number` source: one-hot on the
    /// interval containing the observed value.
    pub fn make_number_interval_writer(
        &mut self,
        channel: &str,
        thresholds: Vec<f64>,
    ) -> Result<IpcNumberIntervalWriter, Box<dyn std::error::Error>> {
        Ok(IpcNumberIntervalWriter::new(
            self.get_or_create_sender(channel),
            thresholds,
        ))
    }

    /// Creates a typed boolean writer that maps `true` → 1.0 and `false` → 0.0.
    pub fn make_boolean_writer(
        &mut self,
        channel: &str,
    ) -> Result<IpcBooleanWriter, Box<dyn std::error::Error>> {
        Ok(IpcBooleanWriter::new(self.get_or_create_sender(channel)))
    }

    /// Creates a timed writer that sends its value at a given frequency.
    pub fn make_timed_writer(
        &mut self,
        channel: &str,
        frequency: f64,
    ) -> Result<Arc<Mutex<Vector>>, Box<dyn std::error::Error>> {
        let writer_tx = self
            .senders
            .entry(channel.to_string())
            .or_insert_with(|| mpsc::channel().0)
            .clone();

        let initial_value = {
            let rc_guard = self.reactive_circuit.lock().unwrap();
            rc_guard
                .leafs
                .iter()
                .find(|l| l.name == channel.strip_prefix('/').unwrap_or(channel))
                .map(|l| l.get_value())
                .unwrap_or_else(|| Vector::zeros(rc_guard.value_size))
        };
        let mut writer = TimedIpcWriter::new(frequency, writer_tx, initial_value)?;

        let value = writer.get_value_access();

        writer.start();
        self.writers.push(writer);

        Ok(value)
    }

    /// Stops and removes all active timed writers.
    pub fn stop_timed_writers(&mut self) {
        self.writers.clear();
    }

    /// Prunes the frequencies of all leaves based on a timestamp threshold.
    pub fn prune_frequencies(&self, threshold: f64, timestamp: Option<f64>) {
        let mut reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        let timestamp = if let Some(ts) = timestamp {
            ts
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        };

        for leaf in &mut reactive_circuit_guard.leafs.iter_mut() {
            leaf.prune_frequency(timestamp, threshold);
        }
    }

    /// Returns a vector of the frequencies of all leaves.
    pub fn get_frequencies(&self) -> Vec<f64> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.get_frequency())
            .collect()
    }

    /// Returns the current value vector for every leaf, in index order.
    pub fn get_values(&self) -> Vec<Vector> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.get_value().clone())
            .collect()
    }

    /// Returns a vector of the names of all leaves.
    pub fn get_names(&self) -> Vec<String> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.name.to_owned())
            .collect()
    }

    /// Returns a `HashMap` mapping leaf names to their indices.
    pub fn get_index_map(&self) -> HashMap<String, usize> {
        self.reactive_circuit
            .lock()
            .unwrap()
            .leafs
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i))
            .collect()
    }
}

impl<S: Semiring> Drop for Manager<S> {
    fn drop(&mut self) {
        self.stop_timed_writers();
    }
}

#[cfg(test)]
mod tests {

    use ndarray::array;

    use super::*;
    use crate::circuit::semiring::LogProb;
    use std::{thread::sleep, time::Duration};

    type TestManager = Manager<LogProb>;

    #[test]
    fn test_read_write() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = TestManager::new(1);

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester_1", array![0.0].into(), 0.0);
        manager.read(receiver, "/test_1", false)?;
        let writer = manager.make_writer("/test_1")?;

        // Wait for long enough that we must have a result
        // The recv_timeout internally can be a bit slow so we wait
        use std::thread::sleep;
        use std::time::Duration;
        sleep(Duration::new(2, 0));

        // Before spinning, value should still be 0.0
        assert_eq!(manager.get_values(), vec![array![0.0]]);

        writer.write(array![1.0].into(), None);
        sleep(Duration::from_millis(20));

        // Leaf should now have value 1.0
        assert_eq!(manager.get_values(), vec![array![1.0]]);

        Ok(())
    }

    #[test]
    fn test_timed_writer() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = TestManager::new(1);
        let receiver = manager.create_leaf("timed_tester", array![0.0].into(), 0.0);
        manager.read(receiver, "timed_tester", false)?;

        // Create a timed writer with a frequency of 100 Hz (sends every 10ms)
        let value_access = manager.make_timed_writer("timed_tester", 100.0)?;

        // Initial value should be 0.0
        assert_eq!(manager.get_values(), vec![array![0.0]]);

        // Update the value that the timed writer sends
        *value_access.lock().unwrap() = array![0.75].into();

        // Wait for a few cycles to ensure the value is sent and received
        sleep(Duration::from_millis(30));

        // The leaf should be updated
        assert_eq!(manager.get_values(), vec![array![0.75]]);

        // The writer is stopped when the manager is dropped.
        manager.stop_timed_writers();

        // Update value again
        *value_access.lock().unwrap() = array![0.25].into();

        // Wait and check that the value is NOT updated because the writer is stopped.
        sleep(Duration::from_millis(30));
        assert_eq!(manager.get_values(), vec![array![0.75]]);

        Ok(())
    }

    #[test]
    fn test_multiple_channels() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = TestManager::new(1);

        let r1 = manager.create_leaf("r1", array![0.0].into(), 0.0);
        let r2 = manager.create_leaf("r2", array![0.0].into(), 0.0);

        manager.read(r1, "/chan1", false)?;
        manager.read(r2, "/chan2", true)?; // This one inverts

        let w1 = manager.make_writer("/chan1")?;
        let w2 = manager.make_writer("/chan2")?;

        assert_eq!(manager.get_values(), vec![array![0.0], array![0.0]]);

        w1.write(array![0.5].into(), None);
        w2.write(array![0.8].into(), None);

        sleep(Duration::from_millis(10));

        let values = manager.get_values();
        assert!((values[0][0] - 0.5_f64).abs() < 1e-9);
        assert!((values[1][0] - 0.2_f64).abs() < 1e-9); // 1.0 - 0.8

        Ok(())
    }

    #[test]
    fn test_prune_frequencies() {
        let mut manager = TestManager::new(1);
        let leaf_idx = manager.create_leaf("freq_leaf", array![0.5].into(), 0.0);
        let mut rc_guard = manager.reactive_circuit.lock().unwrap();
        let leaf = &mut rc_guard.leafs[leaf_idx as usize];

        // Send multiple values at fixed frequence
        for i in 0..100 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            leaf.set_value(array![1.0 / i as f64].into(), now, 1e-3);
            sleep(Duration::from_millis(10));
        }
        drop(rc_guard);

        // Frequency should now be about
        assert!(manager.get_frequencies()[0] - 100.0 < 1e-3);

        // Prune with a threshold of 10s, should not prune
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        manager.prune_frequencies(10.0, Some(now));
        assert!(manager.get_frequencies()[0] - 100.0 < 1e-3);

        // Wait for 1s and prune
        sleep(Duration::from_millis(1000));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        manager.prune_frequencies(1.0, Some(now));
        assert_eq!(manager.get_frequencies()[0], 0.0);
    }

    #[test]
    fn test_getters() {
        let mut manager = TestManager::new(1);
        manager.create_leaf("a", array![0.1].into(), 1.0);
        manager.create_leaf("b", array![0.2].into(), 2.0);

        assert_eq!(manager.get_names(), vec!["a".to_string(), "b".to_string()]);
        let values = manager.get_values();
        assert!((values[0][0] - 0.1_f64).abs() < 1e-9);
        assert!((values[1][0] - 0.2_f64).abs() < 1e-9);
        assert_eq!(manager.get_frequencies(), vec![1.0, 2.0]);

        let index_map = manager.get_index_map();
        assert_eq!(*index_map.get("a").unwrap(), 0);
        assert_eq!(*index_map.get("b").unwrap(), 1);
    }
}
