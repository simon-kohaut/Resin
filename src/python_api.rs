use petgraph::stable_graph::NodeIndex;
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::channels::ipc::{
    IpcBooleanWriter, IpcDensityWriter, IpcNumberWriter, IpcProbabilityWriter, IpcWriter,
    TypedWriter, VectorDistribution,
};
use crate::channels::manager::Manager;
use crate::circuit::leaf::{self, Leaf};
use crate::circuit::reactive::ReactiveCircuit;
use crate::circuit::Vector;
use crate::language::Resin;

/// A wrapper around a shared, mutable `Vector` for timed writers.
#[pyclass(name = "SharedVector")]
struct PySharedVector {
    vec: Arc<Mutex<Vector>>,
}

#[pymethods]
impl PySharedVector {
    /// Sets the value of the shared vector.
    pub fn set(&self, py: Python<'_>, value: Vec<f64>) {
        py.detach(move || {
            *self.vec.lock().unwrap() = Vector::from(value);
        })
    }

    /// Gets the current value of the shared vector.
    pub fn get(&self, py: Python<'_>) -> Vec<f64> {
        py.detach(|| self.vec.lock().unwrap().iter().copied().collect())
    }
}

/// A Python wrapper for `IpcWriter`.
#[pyclass(name = "IpcWriter")]
struct PyIpcWriter {
    writer: IpcWriter,
}

#[pymethods]
impl PyIpcWriter {
    /// Writes a value to the channel.
    pub fn write(&self, py: Python<'_>, value: Vec<f64>, timestamp: Option<f64>) {
        py.detach(|| {
            self.writer.write(Vector::from(value), timestamp);
        })
    }
}

// ---------------------------------------------------------------------------
// Typed writer wrappers
// ---------------------------------------------------------------------------

/// Passes a probability vector straight through to the circuit.
#[pyclass(name = "ProbabilityWriter")]
struct PyProbabilityWriter {
    writer: IpcProbabilityWriter,
}

#[pymethods]
impl PyProbabilityWriter {
    pub fn write(&self, _py: Python<'_>, value: Vec<f64>, timestamp: Option<f64>) {
        self.writer.write(Vector::from(value), timestamp);
    }
}

/// Fan-out density writer.  A single call dispatches to every comparison
/// threshold registered for the source, computing CDF or SF element-wise
/// across all value-space slots (e.g. particle-filter particles).
///
/// Supported distributions and their `params` layout (each inner list is a
/// Vector with one value per particle / value-space slot):
/// - `"normal"`      → `[means, stds]`
/// - `"lognormal"`   → `[log_means, log_stds]`  (natural-log space)
/// - `"exponential"` → `[rates]`
/// - `"uniform"`     → `[lows, highs]`
#[pyclass(name = "DensityWriter")]
struct PyDensityWriter {
    writer: IpcDensityWriter,
}

#[pymethods]
impl PyDensityWriter {
    pub fn write(
        &self,
        _py: Python<'_>,
        distribution: &str,
        params: Vec<Vec<f64>>,
        timestamp: Option<f64>,
    ) -> PyResult<()> {
        let dist = match distribution.to_ascii_lowercase().as_str() {
            "normal" => {
                if params.len() < 2 {
                    return Err(PyValueError::new_err("Normal requires [[means], [stds]]"));
                }
                VectorDistribution::Normal {
                    mean: Vector::from(params[0].clone()),
                    std: Vector::from(params[1].clone()),
                }
            }
            "lognormal" => {
                if params.len() < 2 {
                    return Err(PyValueError::new_err(
                        "LogNormal requires [[log_means], [log_stds]]",
                    ));
                }
                VectorDistribution::LogNormal {
                    log_mean: Vector::from(params[0].clone()),
                    log_std: Vector::from(params[1].clone()),
                }
            }
            "exponential" => {
                if params.is_empty() {
                    return Err(PyValueError::new_err("Exponential requires [[rates]]"));
                }
                VectorDistribution::Exponential {
                    rate: Vector::from(params[0].clone()),
                }
            }
            "uniform" => {
                if params.len() < 2 {
                    return Err(PyValueError::new_err("Uniform requires [[lows], [highs]]"));
                }
                VectorDistribution::Uniform {
                    low: Vector::from(params[0].clone()),
                    high: Vector::from(params[1].clone()),
                }
            }
            other => {
                return Err(PyValueError::new_err(format!(
                    "Unknown distribution '{}'. Supported: normal, lognormal, exponential, uniform",
                    other
                )))
            }
        };
        self.writer.write(&dist, timestamp);
        Ok(())
    }
}

/// Fan-out number writer.  Compares a value vector against every registered
/// threshold element-wise: 1.0 where the comparison holds, else 0.0.
#[pyclass(name = "NumberWriter")]
struct PyNumberWriter {
    writer: IpcNumberWriter,
}

#[pymethods]
impl PyNumberWriter {
    pub fn write(&self, _py: Python<'_>, value: Vec<f64>, timestamp: Option<f64>) {
        self.writer.write(Vector::from(value), timestamp);
    }
}

/// Maps a Python bool to a probability: `True` → 1.0, `False` → 0.0.
#[pyclass(name = "BooleanWriter")]
struct PyBooleanWriter {
    writer: IpcBooleanWriter,
}

#[pymethods]
impl PyBooleanWriter {
    pub fn write(&self, _py: Python<'_>, value: bool, timestamp: Option<f64>) {
        self.writer.write(value, timestamp);
    }
}

/// Manages the state of leaves (Foliage) and the IPC channels for updating them.
#[pyclass(name = "Manager")]
struct PyManager {
    manager: Mutex<Manager>,
}

#[pymethods]
impl PyManager {
    #[new]
    fn new(value_size: usize) -> Self {
        PyManager {
            manager: Mutex::new(Manager::new(value_size)),
        }
    }

    /// Creates a new `Leaf` and returns its index.
    fn create_leaf(&self, py: Python<'_>, name: &str, value: Vec<f64>, frequency: f64) -> u32 {
        let name = name.to_string();
        py.detach(move || {
            let vector_value = Vector::from(value);
            self.manager
                .lock()
                .unwrap()
                .create_leaf(&name, vector_value, frequency)
        })
    }

    /// Creates a reader for a given channel that updates a leaf.
    fn read(&self, py: Python<'_>, receiver_idx: u32, channel: &str, invert: bool) -> PyResult<()> {
        let channel = channel.to_string();
        py.detach(move || {
            self.manager
                .lock()
                .unwrap()
                .read(receiver_idx, &channel, invert)
                .map_err(|e| e.to_string())
        })
        .map_err(|e_str| pyo3::exceptions::PyIOError::new_err(e_str))
    }

    /// Creates a dual reader for a channel that updates two leaves (normal and inverted).
    fn read_dual(
        &self,
        py: Python<'_>,
        receiver_idx_normal: u32,
        receiver_idx_inverted: u32,
        channel: &str,
    ) -> PyResult<()> {
        let channel = channel.to_string();
        py.detach(move || {
            self.manager
                .lock()
                .unwrap()
                .read_dual(receiver_idx_normal, receiver_idx_inverted, &channel)
                .map_err(|e| e.to_string())
        })
        .map_err(|e_str| pyo3::exceptions::PyIOError::new_err(e_str))
    }

    /// Creates a writer for a given channel.
    fn make_writer(&self, py: Python<'_>, channel: &str) -> PyResult<PyIpcWriter> {
        let channel = channel.to_string();
        let writer = py
            .detach(move || {
                self.manager
                    .lock()
                    .unwrap()
                    .make_writer(&channel)
                    .map_err(|e| e.to_string())
            })
            .map_err(|e_str| pyo3::exceptions::PyIOError::new_err(e_str))?;
        Ok(PyIpcWriter { writer })
    }

    /// Creates a timed writer that sends its value at a given frequency.
    fn make_timed_writer(
        &self,
        py: Python<'_>,
        channel: &str,
        frequency: f64,
    ) -> PyResult<PySharedVector> {
        let channel = channel.to_string();
        let value_arc = py
            .detach(move || {
                self.manager
                    .lock()
                    .unwrap()
                    .make_timed_writer(&channel, frequency)
                    .map_err(|e| e.to_string())
            })
            .map_err(|e_str| pyo3::exceptions::PyIOError::new_err(e_str))?;
        Ok(PySharedVector { vec: value_arc })
    }

    /// Stops and removes all active timed writers.
    fn stop_timed_writers(&self, py: Python<'_>) {
        py.detach(|| {
            self.manager.lock().unwrap().stop_timed_writers();
        })
    }

    /// Returns a list of the frequencies of all leaves.
    fn get_frequencies(&self, py: Python<'_>) -> Vec<f64> {
        py.detach(|| self.manager.lock().unwrap().get_frequencies())
    }

    /// Returns a list of the values of all leaves.
    fn get_values(&self, py: Python<'_>) -> Vec<Vec<f64>> {
        py.detach(|| {
            self.manager
                .lock()
                .unwrap()
                .get_values()
                .into_iter()
                .map(|v| v.iter().copied().collect())
                .collect()
        })
    }

    /// Returns a list of the names of all leaves.
    fn get_names(&self, py: Python<'_>) -> Vec<String> {
        py.detach(|| self.manager.lock().unwrap().get_names())
    }
}

/// A Python wrapper for the high-level `Resin` language compiler and runtime.
#[pyclass(name = "Resin")]
struct PyResin {
    /// The full compiled Resin instance, including the comparison registry
    /// needed to build typed writers.
    resin: Arc<Mutex<Resin>>,
}

#[pymethods]
impl PyResin {
    /// Compiles a Resin model string into a runtime instance.
    #[staticmethod]
    fn compile(py: Python<'_>, model: &str, value_size: usize, verbose: bool) -> PyResult<Self> {
        let model = model.to_string();
        let compiled_resin = py
            .detach(move || Resin::compile(&model, value_size, verbose).map_err(|e| e.to_string()))
            .map_err(|e_str| PyRuntimeError::new_err(e_str))?;
        Ok(PyResin {
            resin: Arc::new(Mutex::new(compiled_resin)),
        })
    }

    /// Returns the underlying `ReactiveCircuit` for direct interaction and updates.
    fn get_reactive_circuit(&self) -> PyReactiveCircuit {
        let circuit_arc = self.resin.lock().unwrap().manager.reactive_circuit.clone();
        PyReactiveCircuit {
            circuit: circuit_arc,
        }
    }

    /// Creates a reader for a given channel that updates a leaf.
    fn read(&self, py: Python<'_>, receiver_idx: u32, channel: &str, invert: bool) -> PyResult<()> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        py.detach(move || {
            resin
                .lock()
                .unwrap()
                .manager
                .read(receiver_idx, &channel, invert)
                .map_err(|e| e.to_string())
        })
        .map_err(|e_str| PyIOError::new_err(e_str))
    }

    /// Creates a raw probability writer for a given internal channel name.
    fn make_writer(&self, py: Python<'_>, channel: &str) -> PyResult<PyIpcWriter> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        let writer = py
            .detach(move || {
                resin
                    .lock()
                    .unwrap()
                    .manager
                    .make_writer(&channel)
                    .map_err(|e| e.to_string())
            })
            .map_err(|e_str| PyIOError::new_err(e_str))?;
        Ok(PyIpcWriter { writer })
    }

    /// Returns the correctly typed writer for a declared source atom.
    /// The returned object is one of `ProbabilityWriter`, `DensityWriter`,
    /// `NumberWriter`, or `BooleanWriter`.
    fn make_writer_for(&self, py: Python<'_>, source_name: &str) -> PyResult<PyObject> {
        let source_name = source_name.to_string();
        let resin = self.resin.clone();
        let typed_writer = py
            .detach(move || {
                resin
                    .lock()
                    .unwrap()
                    .make_writer_for(&source_name)
                    .map_err(|e| e.to_string())
            })
            .map_err(|e_str| PyRuntimeError::new_err(e_str))?;

        match typed_writer {
            TypedWriter::Probability(w) => {
                Ok(Py::new(py, PyProbabilityWriter { writer: w })?.into_any())
            }
            TypedWriter::Density(w) => {
                Ok(Py::new(py, PyDensityWriter { writer: w })?.into_any())
            }
            TypedWriter::Number(w) => {
                Ok(Py::new(py, PyNumberWriter { writer: w })?.into_any())
            }
            TypedWriter::Boolean(w) => {
                Ok(Py::new(py, PyBooleanWriter { writer: w })?.into_any())
            }
        }
    }

    /// Creates a timed writer that sends its value at a given frequency.
    fn make_timed_writer(
        &self,
        py: Python<'_>,
        channel: &str,
        frequency: f64,
    ) -> PyResult<PySharedVector> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        let value_arc = py
            .detach(move || {
                resin
                    .lock()
                    .unwrap()
                    .manager
                    .make_timed_writer(&channel, frequency)
                    .map_err(|e| e.to_string())
            })
            .map_err(|e_str| PyIOError::new_err(e_str))?;
        Ok(PySharedVector { vec: value_arc })
    }

    /// Stops and removes all active timed writers associated with this Resin instance.
    fn stop_timed_writers(&self, py: Python<'_>) {
        let resin = self.resin.clone();
        py.detach(move || {
            resin.lock().unwrap().manager.stop_timed_writers();
        })
    }

    /// Returns a list of the names of all leaves in the compiled circuit.
    fn get_names(&self, py: Python<'_>) -> Vec<String> {
        let resin = self.resin.clone();
        py.detach(move || resin.lock().unwrap().manager.get_names())
    }

    /// Returns a list of the frequencies of all leaves.
    fn get_frequencies(&self, py: Python<'_>) -> Vec<f64> {
        let resin = self.resin.clone();
        py.detach(move || resin.lock().unwrap().manager.get_frequencies())
    }

    /// Returns a list of the current values of all leaves.
    fn get_values(&self, py: Python<'_>) -> Vec<Vec<f64>> {
        let resin = self.resin.clone();
        py.detach(move || {
            resin
                .lock()
                .unwrap()
                .manager
                .get_values()
                .into_iter()
                .map(|v| v.iter().copied().collect())
                .collect()
        })
    }
}

#[pyclass(name = "ReactiveCircuit")]
struct PyReactiveCircuit {
    circuit: Arc<Mutex<ReactiveCircuit>>,
}

#[pymethods]
impl PyReactiveCircuit {
    #[new]
    fn new(value_size: usize) -> PyResult<Self> {
        Ok(PyReactiveCircuit {
            circuit: Arc::new(Mutex::new(ReactiveCircuit::new(value_size))),
        })
    }

    #[staticmethod]
    fn from_sum_product(
        value_size: usize,
        sum_product: Vec<Vec<u32>>,
        target_token: String,
    ) -> PyResult<Self> {
        Ok(PyReactiveCircuit {
            circuit: Arc::new(Mutex::new(ReactiveCircuit::from_sum_product(
                value_size,
                &sum_product,
                target_token,
            ))),
        })
    }

    fn add_leaf(
        &self,
        py: Python<'_>,
        initial_value: Vec<f64>,
        initial_timestamp: f64,
        token: String,
    ) -> PyResult<usize> {
        Ok(py.detach(move || {
            let mut circuit = self.circuit.lock().unwrap();
            let leaf_index = circuit.leafs.len();
            let vector_value = Vector::from(initial_value);
            circuit
                .leafs
                .push(Leaf::new(vector_value, initial_timestamp, &token));
            leaf_index
        }))
    }

    fn update_leaf(
        &self,
        py: Python<'_>,
        leaf_index: u32,
        new_value: Vec<f64>,
        timestamp: f64,
    ) -> PyResult<()> {
        py.detach(move || {
            let mut circuit = self.circuit.lock().unwrap();
            let vector_value = Vector::from(new_value);
            leaf::update(&mut circuit, leaf_index, vector_value, timestamp);
        });
        Ok(())
    }

    fn add_sum_product(&self, py: Python<'_>, sum_product: Vec<Vec<u32>>, target_token: &str) {
        let target_token = target_token.to_string();
        py.detach(move || {
            self.circuit
                .lock()
                .unwrap()
                .add_sum_product(&sum_product, &target_token);
        })
    }

    fn adapt(&self, py: Python<'_>, bin_size: f64, number_bins: usize) {
        py.detach(move || {
            let boundaries = crate::channels::clustering::create_boundaries(bin_size, number_bins);
            self.circuit.lock().unwrap().adapt(&boundaries);
        })
    }

    fn update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let results = py.detach(move || self.circuit.lock().unwrap().update());
        let dict = PyDict::new(py);
        for (token, vector) in results {
            // TODO: consider using `rust-numpy`
            let py_vec: Vec<f64> = vector.iter().copied().collect();
            dict.set_item(token, py_vec)?;
        }
        Ok(dict.into())
    }

    fn full_update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let results = py.detach(move || self.circuit.lock().unwrap().full_update());
        let dict = PyDict::new(py);
        for (token, vector) in results {
            let py_vec: Vec<f64> = vector.iter().copied().collect();
            dict.set_item(token, py_vec)?;
        }
        Ok(dict.into())
    }

    fn lift_leaf(&self, py: Python<'_>, index: u32) {
        py.detach(move || {
            self.circuit.lock().unwrap().lift_leaf(index);
        })
    }

    fn drop_leaf(&self, py: Python<'_>, index: u32) {
        py.detach(move || {
            self.circuit.lock().unwrap().drop_leaf(index);
        })
    }

    fn to_dot(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let path = path.to_string();
        py.detach(move || self.circuit.lock().unwrap().to_dot(&path))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    fn to_svg(&self, py: Python<'_>, path: &str, keep_dot: bool) -> PyResult<()> {
        let path = path.to_string();
        py.detach(move || self.circuit.lock().unwrap().to_svg(&path, keep_dot))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    fn to_combined_svg(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let path = path.to_string();
        py.detach(move || self.circuit.lock().unwrap().to_combined_svg(&path))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }
}

#[pymodule]
fn resin(_py: Python<'_>, m: Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyResin>()?;
    m.add_class::<PyReactiveCircuit>()?;
    m.add_class::<PyManager>()?;
    m.add_class::<PySharedVector>()?;
    m.add_class::<PyIpcWriter>()?;
    m.add_class::<PyProbabilityWriter>()?;
    m.add_class::<PyDensityWriter>()?;
    m.add_class::<PyNumberWriter>()?;
    m.add_class::<PyBooleanWriter>()?;
    Ok(())
}
