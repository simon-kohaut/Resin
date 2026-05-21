use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::{Arc, Mutex};

use crate::channels::ipc::{
    IpcBooleanWriter, IpcCategoricalWriter, IpcDensityWriter, IpcNumberWriter,
    IpcProbabilityWriter, TypedWriter, VectorDistribution,
};
use crate::circuit::leaf::{self, Leaf};
use crate::circuit::reactive::ReactiveCircuit;
use crate::circuit::semiring::{Boolean, Fuzzy, LogProb, MaxProduct, ProbGradient};
use crate::circuit::Vector;
use crate::language::Resin;

// ---------------------------------------------------------------------------
// Semiring dispatch
// ---------------------------------------------------------------------------

/// Holds a compiled `Resin` instance for any supported semiring.
enum ResinVariant {
    LogProb(Resin<LogProb>),
    MaxProduct(Resin<MaxProduct>),
    Fuzzy(Resin<Fuzzy>),
    Boolean(Resin<Boolean>),
    ProbGradient(Resin<ProbGradient>),
}

/// Holds a shared `ReactiveCircuit` handle for any supported semiring.
#[derive(Clone)]
enum RCVariant {
    LogProb(Arc<Mutex<ReactiveCircuit<LogProb>>>),
    MaxProduct(Arc<Mutex<ReactiveCircuit<MaxProduct>>>),
    Fuzzy(Arc<Mutex<ReactiveCircuit<Fuzzy>>>),
    Boolean(Arc<Mutex<ReactiveCircuit<Boolean>>>),
    ProbGradient(Arc<Mutex<ReactiveCircuit<ProbGradient>>>),
}

/// Dispatch a method call over all `ResinVariant` arms.
/// `$guard` must be a `MutexGuard<ResinVariant>`; `$r` is bound as `&mut Resin<S>`.
macro_rules! with_resin {
    ($guard:expr, $r:ident => $body:expr) => {
        match &mut *$guard {
            ResinVariant::LogProb($r) => $body,
            ResinVariant::MaxProduct($r) => $body,
            ResinVariant::Fuzzy($r) => $body,
            ResinVariant::Boolean($r) => $body,
            ResinVariant::ProbGradient($r) => $body,
        }
    };
}

/// Dispatch a method call over all `RCVariant` arms.
/// `$variant` is consumed; `$c` is bound as `MutexGuard<ReactiveCircuit<S>>`.
macro_rules! with_rc {
    ($variant:expr, $c:ident => $body:expr) => {
        match $variant {
            #[allow(unused_mut)]
            RCVariant::LogProb(arc) => {
                let mut $c = arc.lock().unwrap();
                $body
            }
            #[allow(unused_mut)]
            RCVariant::MaxProduct(arc) => {
                let mut $c = arc.lock().unwrap();
                $body
            }
            #[allow(unused_mut)]
            RCVariant::Fuzzy(arc) => {
                let mut $c = arc.lock().unwrap();
                $body
            }
            #[allow(unused_mut)]
            RCVariant::Boolean(arc) => {
                let mut $c = arc.lock().unwrap();
                $body
            }
            #[allow(unused_mut)]
            RCVariant::ProbGradient(arc) => {
                let mut $c = arc.lock().unwrap();
                $body
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Typed writer wrappers
// ---------------------------------------------------------------------------

/// A wrapper around a shared, mutable `Vector` for timed writers.
#[pyclass(name = "SharedVector")]
struct PySharedVector {
    vec: Arc<Mutex<Vector>>,
}

#[pymethods]
impl PySharedVector {
    pub fn set(&self, py: Python<'_>, value: Vec<f64>) {
        py.detach(move || {
            *self.vec.lock().unwrap() = Vector::from(value);
        })
    }

    pub fn get(&self, py: Python<'_>) -> Vec<f64> {
        py.detach(|| self.vec.lock().unwrap().iter().copied().collect())
    }
}

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

/// Sends a flat probability matrix `[col₀, col₁, …]` where each column has
/// `value_size` entries, one per batch slot.  Accepts a flat list of length
/// `n_categories * value_size`.
#[pyclass(name = "CategoricalWriter")]
struct PyCategoricalWriter {
    writer: IpcCategoricalWriter,
}

#[pymethods]
impl PyCategoricalWriter {
    pub fn write(&self, _py: Python<'_>, probabilities: Vec<f64>, timestamp: Option<f64>) {
        self.writer.write(Vector::from(probabilities), timestamp);
    }

    pub fn n_categories(&self) -> usize {
        self.writer.n_categories()
    }
    pub fn value_size(&self) -> usize {
        self.writer.value_size()
    }
}

/// Converts a `TypedWriter` into the appropriate Python writer object.
fn typed_writer_to_py(py: Python<'_>, writer: TypedWriter) -> PyResult<Py<PyAny>> {
    match writer {
        TypedWriter::Probability(w) => {
            Ok(Py::new(py, PyProbabilityWriter { writer: w })?.into_any())
        }
        TypedWriter::Density(w) => Ok(Py::new(py, PyDensityWriter { writer: w })?.into_any()),
        TypedWriter::Number(w) => Ok(Py::new(py, PyNumberWriter { writer: w })?.into_any()),
        TypedWriter::Boolean(w) => Ok(Py::new(py, PyBooleanWriter { writer: w })?.into_any()),
        TypedWriter::Categorical(w) => {
            Ok(Py::new(py, PyCategoricalWriter { writer: w })?.into_any())
        }
    }
}

// ---------------------------------------------------------------------------
// PyResin
// ---------------------------------------------------------------------------

/// A Python wrapper for the high-level `Resin` language compiler and runtime.
#[pyclass(name = "Resin")]
struct PyResin {
    resin: Arc<Mutex<ResinVariant>>,
}

#[pymethods]
impl PyResin {
    /// Compiles a Resin model string into a runtime instance.
    ///
    /// `semiring` selects the inference algebra.  Supported values (case-insensitive):
    /// `"LogProb"` (default), `"MaxProduct"`, `"Fuzzy"`, `"Boolean"`, `"ProbGradient"`.
    #[staticmethod]
    #[pyo3(signature = (model, value_size=1, verbose=false, semiring=None, update_threshold=1e-3))]
    fn compile(
        py: Python<'_>,
        model: &str,
        value_size: usize,
        verbose: bool,
        semiring: Option<&str>,
        update_threshold: f64,
    ) -> PyResult<Self> {
        let model = model.to_string();
        let semiring = semiring.unwrap_or("logprob").to_ascii_lowercase();
        let variant = py
            .detach(move || -> Result<ResinVariant, String> {
                match semiring.as_str() {
                    "logprob" | "log_prob" => {
                        Resin::<LogProb>::compile(&model, value_size, update_threshold, verbose)
                            .map(|r| { r.manager.reactive_circuit.lock().unwrap().update_threshold = update_threshold; r })
                            .map(ResinVariant::LogProb)
                            .map_err(|e| e.to_string())
                    }
                    "maxproduct" | "max_product" => {
                        Resin::<MaxProduct>::compile(&model, value_size, update_threshold, verbose)
                            .map(|r| { r.manager.reactive_circuit.lock().unwrap().update_threshold = update_threshold; r })
                            .map(ResinVariant::MaxProduct)
                            .map_err(|e| e.to_string())
                    }
                    "fuzzy" => Resin::<Fuzzy>::compile(&model, value_size, update_threshold, verbose)
                        .map(|r| { r.manager.reactive_circuit.lock().unwrap().update_threshold = update_threshold; r })
                        .map(ResinVariant::Fuzzy)
                        .map_err(|e| e.to_string()),
                    "boolean" => Resin::<Boolean>::compile(&model, value_size, update_threshold, verbose)
                        .map(|r| { r.manager.reactive_circuit.lock().unwrap().update_threshold = update_threshold; r })
                        .map(ResinVariant::Boolean)
                        .map_err(|e| e.to_string()),
                    "probgradient" | "prob_gradient" => {
                        Resin::<ProbGradient>::compile(&model, value_size, update_threshold, verbose)
                            .map(|r| { r.manager.reactive_circuit.lock().unwrap().update_threshold = update_threshold; r })
                            .map(ResinVariant::ProbGradient)
                            .map_err(|e| e.to_string())
                    }
                    other => Err(format!(
                        "Unknown semiring '{other}'. \
                         Supported: LogProb, MaxProduct, Fuzzy, Boolean, ProbGradient"
                    )),
                }
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok(PyResin {
            resin: Arc::new(Mutex::new(variant)),
        })
    }

    fn get_reactive_circuit(&self) -> PyReactiveCircuit {
        let circuit = match &*self.resin.lock().unwrap() {
            ResinVariant::LogProb(r) => RCVariant::LogProb(r.manager.reactive_circuit.clone()),
            ResinVariant::MaxProduct(r) => {
                RCVariant::MaxProduct(r.manager.reactive_circuit.clone())
            }
            ResinVariant::Fuzzy(r) => RCVariant::Fuzzy(r.manager.reactive_circuit.clone()),
            ResinVariant::Boolean(r) => RCVariant::Boolean(r.manager.reactive_circuit.clone()),
            ResinVariant::ProbGradient(r) => {
                RCVariant::ProbGradient(r.manager.reactive_circuit.clone())
            }
        };
        PyReactiveCircuit { circuit }
    }

    fn read(&self, py: Python<'_>, receiver_idx: u32, channel: &str, invert: bool) -> PyResult<()> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.read(receiver_idx, &channel, invert).map_err(|e| e.to_string()))
        })
        .map_err(PyIOError::new_err)
    }

    fn make_writer(&self, py: Python<'_>, channel: &str) -> PyResult<Py<PyAny>> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        let typed_writer = py
            .detach(move || {
                let mut guard = resin.lock().unwrap();
                with_resin!(guard, r => r.make_writer(&channel).map_err(|e| e.to_string()))
            })
            .map_err(PyRuntimeError::new_err)?;
        typed_writer_to_py(py, typed_writer)
    }

    fn make_writer_for(&self, py: Python<'_>, source_name: &str) -> PyResult<Py<PyAny>> {
        let source_name = source_name.to_string();
        let resin = self.resin.clone();
        let typed_writer = py
            .detach(move || {
                let mut guard = resin.lock().unwrap();
                with_resin!(guard, r => r.make_writer_for(&source_name).map_err(|e| e.to_string()))
            })
            .map_err(PyRuntimeError::new_err)?;
        typed_writer_to_py(py, typed_writer)
    }

    fn make_categorical_writer(&self, py: Python<'_>, channel: &str) -> PyResult<Py<PyAny>> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        let typed_writer = py
            .detach(move || {
                let mut guard = resin.lock().unwrap();
                with_resin!(guard, r => r.make_categorical_writer(&channel).map_err(|e| e.to_string()))
            })
            .map_err(PyRuntimeError::new_err)?;
        typed_writer_to_py(py, typed_writer)
    }

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
                let mut guard = resin.lock().unwrap();
                with_resin!(guard, r => r.manager.make_timed_writer(&channel, frequency).map_err(|e| e.to_string()))
            })
            .map_err(PyIOError::new_err)?;
        Ok(PySharedVector { vec: value_arc })
    }

    fn stop_timed_writers(&self, py: Python<'_>) {
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.stop_timed_writers())
        })
    }

    fn get_names(&self, py: Python<'_>) -> Vec<String> {
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.get_names())
        })
    }

    fn get_frequencies(&self, py: Python<'_>) -> Vec<f64> {
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.get_frequencies())
        })
    }

    fn get_values(&self, py: Python<'_>) -> Vec<Vec<f64>> {
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager
                .get_values()
                .into_iter()
                .map(|v| v.iter().copied().collect())
                .collect())
        })
    }

    /// Returns the parameter groups discovered during compilation.
    ///
    /// Each key is `"{predicate}/{clause_index}"` and maps to the names of the
    /// positive-polarity cause leaves sharing that `P(...)` value.
    /// Returns the gradients for a single source looked up by **atom name**.
    ///
    /// `gradients` is the inner `"gradients"` dict from a `gradient_update` /
    /// `full_gradient_update` result (i.e. `{leaf_name: float}`).
    /// Raises `RuntimeError` if the semiring is not `ProbGradient`.
    fn source_gradients_for(
        &self,
        py: Python<'_>,
        gradients: std::collections::HashMap<String, f64>,
        atom_name: &str,
    ) -> PyResult<Py<PyDict>> {
        Self::source_gradients_impl(py, self.resin.clone(), gradients, atom_name.to_string())
    }

    /// Returns the gradients for a single source looked up by **channel name**.
    ///
    /// `gradients` is the inner `"gradients"` dict from a `gradient_update` /
    /// `full_gradient_update` result (i.e. `{leaf_name: float}`).
    /// Raises `RuntimeError` if the semiring is not `ProbGradient`.
    fn source_gradients(
        &self,
        py: Python<'_>,
        gradients: std::collections::HashMap<String, f64>,
        channel: &str,
    ) -> PyResult<Py<PyDict>> {
        Self::source_gradients_impl(py, self.resin.clone(), gradients, channel.to_string())
    }

    fn get_parameter_groups(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let resin = self.resin.clone();
        let groups = py.detach(move || {
            let guard = resin.lock().unwrap();
            match &*guard {
                ResinVariant::LogProb(r) => r.get_parameter_groups().clone(),
                ResinVariant::MaxProduct(r) => r.get_parameter_groups().clone(),
                ResinVariant::Fuzzy(r) => r.get_parameter_groups().clone(),
                ResinVariant::Boolean(r) => r.get_parameter_groups().clone(),
                ResinVariant::ProbGradient(r) => r.get_parameter_groups().clone(),
            }
        });
        let dict = PyDict::new(py);
        for (key, names) in groups {
            dict.set_item(key, names)?;
        }
        Ok(dict.into())
    }

    /// Applies one gradient-descent step to `P(...)` clause parameters.
    ///
    /// Aggregates gradients across all groundings of each parameter group,
    /// then applies a single shared update so all groundings stay in sync.
    /// Source atoms and comparison atoms are never modified.
    ///
    /// Raises `RuntimeError` if the semiring is not `ProbGradient`.
    fn fit_parameters(
        &self,
        py: Python<'_>,
        gradients: std::collections::HashMap<String, f64>,
        lr: f64,
        loss: f64,
        parameters: Option<Vec<String>>,
        timestamp: f64,
    ) -> PyResult<()> {
        let resin = self.resin.clone();
        py.detach(move || match &mut *resin.lock().unwrap() {
            ResinVariant::ProbGradient(r) => {
                let params_ref: Option<Vec<&str>> = parameters
                    .as_ref()
                    .map(|v| v.iter().map(|s| s.as_str()).collect());
                r.fit_parameters(&gradients, lr, loss, params_ref.as_deref(), timestamp);
                Ok(())
            }
            _ => Err("fit_parameters requires the ProbGradient semiring".to_string()),
        })
        .map_err(PyRuntimeError::new_err)
    }
}

impl PyResin {
    fn source_gradients_impl(
        py: Python<'_>,
        resin: Arc<Mutex<ResinVariant>>,
        gradients: std::collections::HashMap<String, f64>,
        name: String,
    ) -> PyResult<Py<PyDict>> {
        let result = py
            .detach(move || match &*resin.lock().unwrap() {
                ResinVariant::ProbGradient(r) => Ok(r
                    .source_gradients(&gradients, &name)
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect::<std::collections::HashMap<String, f64>>()),
                _ => Err("source_gradients requires the ProbGradient semiring".to_string()),
            })
            .map_err(PyRuntimeError::new_err)?;
        let dict = PyDict::new(py);
        for (leaf, grad) in result {
            dict.set_item(leaf, grad)?;
        }
        Ok(dict.into())
    }
}

// ---------------------------------------------------------------------------
// PyReactiveCircuit
// ---------------------------------------------------------------------------

#[pyclass(name = "ReactiveCircuit")]
struct PyReactiveCircuit {
    circuit: RCVariant,
}

#[pymethods]
impl PyReactiveCircuit {
    #[new]
    #[pyo3(signature = (value_size, update_threshold=1e-3))]
    fn new(value_size: usize, update_threshold: f64) -> PyResult<Self> {
        let mut rc = ReactiveCircuit::new(value_size);
        rc.update_threshold = update_threshold;
        Ok(PyReactiveCircuit {
            circuit: RCVariant::LogProb(Arc::new(Mutex::new(rc))),
        })
    }

    #[staticmethod]
    fn from_sum_product(
        value_size: usize,
        sum_product: Vec<Vec<u32>>,
        target_token: String,
    ) -> PyResult<Self> {
        Ok(PyReactiveCircuit {
            circuit: RCVariant::LogProb(Arc::new(Mutex::new(ReactiveCircuit::from_sum_product(
                value_size,
                &sum_product,
                target_token,
            )))),
        })
    }

    fn add_leaf(
        &self,
        py: Python<'_>,
        initial_value: Vec<f64>,
        initial_timestamp: f64,
        token: String,
    ) -> PyResult<usize> {
        let circuit = self.circuit.clone();
        Ok(py.detach(move || {
            with_rc!(circuit, c => {
                let leaf_index = c.leafs.len();
                c.leafs.push(Leaf::new(Vector::from(initial_value), initial_timestamp, &token, leaf_index));
                leaf_index
            })
        }))
    }

    fn update_leaf(
        &self,
        py: Python<'_>,
        leaf_index: u32,
        new_value: Vec<f64>,
        timestamp: f64,
    ) -> PyResult<()> {
        let circuit = self.circuit.clone();
        py.detach(move || {
            with_rc!(circuit, c => leaf::update(&mut c, leaf_index, Vector::from(new_value), timestamp))
        });
        Ok(())
    }

    fn add_sum_product(&self, py: Python<'_>, sum_product: Vec<Vec<u32>>, target_token: &str) {
        let target_token = target_token.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.add_sum_product(&sum_product, &target_token)))
    }

    fn adapt(&self, py: Python<'_>, bin_size: f64, number_bins: usize) {
        let circuit = self.circuit.clone();
        py.detach(move || {
            let boundaries = crate::channels::clustering::create_boundaries(bin_size, number_bins);
            with_rc!(circuit, c => c.adapt(&boundaries))
        })
    }

    fn update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let circuit = self.circuit.clone();
        let results = py.detach(move || with_rc!(circuit, c => c.update()));
        let dict = PyDict::new(py);
        for (token, vector) in results {
            dict.set_item(token, vector.iter().copied().collect::<Vec<_>>())?;
        }
        Ok(dict.into())
    }

    fn full_update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let circuit = self.circuit.clone();
        let results = py.detach(move || with_rc!(circuit, c => c.full_update()));
        let dict = PyDict::new(py);
        for (token, vector) in results {
            dict.set_item(token, vector.iter().copied().collect::<Vec<_>>())?;
        }
        Ok(dict.into())
    }

    /// Reactive update with gradients unpacked by leaf name.
    /// Raises `RuntimeError` if the semiring is not `ProbGradient`.
    fn gradient_update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let circuit = self.circuit.clone();
        let unpacked = py
            .detach(move || match circuit {
                RCVariant::ProbGradient(arc) => Ok(arc.lock().unwrap().gradient_update()),
                _ => Err("gradient_update requires the ProbGradient semiring".to_string()),
            })
            .map_err(PyRuntimeError::new_err)?;
        Self::gradients_to_py(py, unpacked)
    }

    /// Full (invalidating) update with gradients unpacked by leaf name.
    /// Raises `RuntimeError` if the semiring is not `ProbGradient`.
    fn full_gradient_update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let circuit = self.circuit.clone();
        let unpacked = py
            .detach(move || match circuit {
                RCVariant::ProbGradient(arc) => Ok(arc.lock().unwrap().full_gradient_update()),
                _ => Err("full_gradient_update requires the ProbGradient semiring".to_string()),
            })
            .map_err(PyRuntimeError::new_err)?;
        Self::gradients_to_py(py, unpacked)
    }

    /// One gradient-descent step over leaf probabilities.
    /// Raises `RuntimeError` if the semiring is not `ProbGradient`.
    fn fit(
        &self,
        py: Python<'_>,
        gradients: std::collections::HashMap<String, f64>,
        lr: f64,
        loss: f64,
        atoms: Option<Vec<String>>,
        timestamp: f64,
    ) -> PyResult<()> {
        let circuit = self.circuit.clone();
        py.detach(move || match circuit {
            RCVariant::ProbGradient(arc) => {
                arc.lock()
                    .unwrap()
                    .fit(&gradients, lr, loss, atoms.as_deref(), timestamp);
                Ok(())
            }
            _ => Err("fit requires the ProbGradient semiring".to_string()),
        })
        .map_err(PyRuntimeError::new_err)
    }

    fn lift_leaf(&self, py: Python<'_>, index: u32) {
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.lift_leaf(index)))
    }

    fn drop_leaf(&self, py: Python<'_>, index: u32) {
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.drop_leaf(index)))
    }

    fn to_dot(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let path = path.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.to_dot(&path)))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    fn to_svg(&self, py: Python<'_>, path: &str, keep_dot: bool) -> PyResult<()> {
        let path = path.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.to_svg(&path, keep_dot)))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    fn to_combined_svg(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let path = path.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.to_combined_svg(&path)))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }
}

impl PyReactiveCircuit {
    fn gradients_to_py(
        py: Python<'_>,
        unpacked: std::collections::HashMap<String, (f64, std::collections::HashMap<String, f64>)>,
    ) -> PyResult<Py<PyDict>> {
        let outer = PyDict::new(py);
        for (target, (wmc, gradients)) in unpacked {
            let inner = PyDict::new(py);
            inner.set_item("probability", wmc)?;
            let grad_dict = PyDict::new(py);
            for (name, grad) in gradients {
                grad_dict.set_item(name, grad)?;
            }
            inner.set_item("gradients", grad_dict)?;
            outer.set_item(target, inner)?;
        }
        Ok(outer.into())
    }
}

#[pymodule]
fn resin(_py: Python<'_>, m: Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyResin>()?;
    m.add_class::<PyReactiveCircuit>()?;
    m.add_class::<PySharedVector>()?;
    m.add_class::<PyProbabilityWriter>()?;
    m.add_class::<PyDensityWriter>()?;
    m.add_class::<PyNumberWriter>()?;
    m.add_class::<PyBooleanWriter>()?;
    m.add_class::<PyCategoricalWriter>()?;
    Ok(())
}
