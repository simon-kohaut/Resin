use numpy::{PyArray1, PyArrayLike1};
use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::{Arc, Mutex};

use crate::channels::ipc::{
    IpcBooleanWriter, IpcCategoricalWriter, IpcDensityIntervalWriter, IpcNumberIntervalWriter,
    IpcProbabilityWriter, TypedWriter, VectorDistribution,
};
use crate::circuit::leaf;
use crate::circuit::reactive::{ReactiveCircuit, Topology};
use crate::circuit::semiring::{
    Boolean, CriticalExplanation, Fuzzy, LogProb, MaxProduct, ProbGradient, Semiring,
};
use crate::circuit::Vector;
use crate::language::Resin;

// ---------------------------------------------------------------------------
// Fast numpy <-> Vector conversion
// ---------------------------------------------------------------------------

/// Accepts a numpy `float64` array (bulk-copied in one pass, no per-element
/// boxing) or, as a fallback, any Python sequence of floats (e.g. a plain
/// `list`), so existing call sites keep working.
///
/// Bridges `PyArrayLike1<f64>` to `Vector` (`ArcArray1<f64>`): the two types
/// aren't directly convertible since one comes from the `numpy` crate and the
/// other is an `ndarray` type alias defined in this crate.
fn array_like_to_vector(value: PyArrayLike1<'_, f64>) -> Vector {
    match value.as_array().as_slice() {
        Some(slice) => Vector::from(slice.to_vec()),
        None => Vector::from(value.as_array().to_vec()),
    }
}

/// Converts a `Vector` into a numpy array in one bulk copy for return to Python.
fn vector_to_pyarray<'py>(py: Python<'py>, vector: &Vector) -> Bound<'py, PyArray1<f64>> {
    match vector.as_slice() {
        Some(slice) => PyArray1::from_slice(py, slice),
        None => PyArray1::from_iter(py, vector.iter().copied()),
    }
}

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
    CriticalExplanation(Resin<CriticalExplanation>),
}

/// Holds a shared `ReactiveCircuit` handle for any supported semiring.
#[derive(Clone)]
enum RCVariant {
    LogProb(Arc<Mutex<ReactiveCircuit<LogProb>>>),
    MaxProduct(Arc<Mutex<ReactiveCircuit<MaxProduct>>>),
    Fuzzy(Arc<Mutex<ReactiveCircuit<Fuzzy>>>),
    Boolean(Arc<Mutex<ReactiveCircuit<Boolean>>>),
    ProbGradient(Arc<Mutex<ReactiveCircuit<ProbGradient>>>),
    CriticalExplanation(Arc<Mutex<ReactiveCircuit<CriticalExplanation>>>),
}

fn topology(dag: bool) -> Topology {
    if dag {
        Topology::Dag
    } else {
        Topology::Tree
    }
}

/// Calls `$callback!(Semiring)` for the semiring named `$name` (case-insensitive),
/// or returns an error naming the supported semirings. Semiring types and the
/// `ResinVariant` / `RCVariant` arms share their names.
macro_rules! with_semiring {
    ($name:expr, $callback:ident) => {
        match $name.to_ascii_lowercase().as_str() {
            "logprob" | "log_prob" => $callback!(LogProb),
            "maxproduct" | "max_product" => $callback!(MaxProduct),
            "fuzzy" => $callback!(Fuzzy),
            "boolean" => $callback!(Boolean),
            "probgradient" | "prob_gradient" => $callback!(ProbGradient),
            "criticalexplanation" | "critical_explanation" => $callback!(CriticalExplanation),
            other => Err(format!(
                "Unknown semiring '{other}'. \
                 Supported: LogProb, MaxProduct, Fuzzy, Boolean, ProbGradient, \
                 CriticalExplanation"
            )),
        }
    };
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
            ResinVariant::CriticalExplanation($r) => $body,
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
            #[allow(unused_mut)]
            RCVariant::CriticalExplanation(arc) => {
                let mut $c = arc.lock().unwrap();
                $body
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Typed writer wrappers
// ---------------------------------------------------------------------------

/// The value of a timed writer (see `Resin.make_timed_writer`), which sends it
/// to its channel at a fixed frequency.
#[pyclass(name = "SharedVector")]
struct PySharedVector {
    vec: Arc<Mutex<Vector>>,
}

#[pymethods]
impl PySharedVector {
    /// Replaces the value that the timed writer sends from now on.
    pub fn set(&self, py: Python<'_>, value: PyArrayLike1<'_, f64>) {
        let value = array_like_to_vector(value);
        py.detach(move || {
            *self.vec.lock().unwrap() = value;
        })
    }

    /// Returns the value that the timed writer currently sends.
    pub fn get<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        let vector = py.detach(|| self.vec.lock().unwrap().clone());
        vector_to_pyarray(py, &vector)
    }
}

/// Writer for a `Probability` source: passes probabilities straight through to
/// the circuit.
#[pyclass(name = "ProbabilityWriter")]
struct PyProbabilityWriter {
    writer: IpcProbabilityWriter,
}

#[pymethods]
impl PyProbabilityWriter {
    /// Sends `value`, one probability in `[0, 1]` per value slot (length
    /// `value_size`). `timestamp` is in seconds; `None` uses the current Unix time.
    #[pyo3(signature = (value, timestamp=None))]
    pub fn write(&self, _py: Python<'_>, value: PyArrayLike1<'_, f64>, timestamp: Option<f64>) {
        self.writer.write(array_like_to_vector(value), timestamp);
    }
}

/// Writer for a `Density` source. A single call dispatches to every comparison
/// threshold registered for the source, computing CDF or SF element-wise
/// across all value slots (e.g. particle-filter particles).
///
/// Supported distributions and their `params` layout (each inner list is a
/// Vector with one value per particle / value-space slot):
/// - `"normal"`      → `[means, stds]`
/// - `"lognormal"`   → `[log_means, log_stds]`  (natural-log space)
/// - `"exponential"` → `[rates]`
/// - `"uniform"`     → `[lows, highs]`
#[pyclass(name = "DensityWriter")]
struct PyDensityWriter {
    writer: IpcDensityIntervalWriter,
}

#[pymethods]
impl PyDensityWriter {
    /// Sends a distribution, e.g. `write("normal", [[25.0], [5.0]])`.
    /// `timestamp` is in seconds; `None` uses the current Unix time.
    /// Raises `ValueError` for an unknown distribution or missing parameters.
    #[pyo3(signature = (distribution, params, timestamp=None))]
    pub fn write(
        &self,
        _py: Python<'_>,
        distribution: &str,
        params: Vec<PyArrayLike1<'_, f64>>,
        timestamp: Option<f64>,
    ) -> PyResult<()> {
        let params: Vec<Vector> = params.into_iter().map(array_like_to_vector).collect();
        let dist = match distribution.to_ascii_lowercase().as_str() {
            "normal" => {
                if params.len() < 2 {
                    return Err(PyValueError::new_err("Normal requires [[means], [stds]]"));
                }
                VectorDistribution::Normal {
                    mean: params[0].clone(),
                    std: params[1].clone(),
                }
            }
            "lognormal" => {
                if params.len() < 2 {
                    return Err(PyValueError::new_err(
                        "LogNormal requires [[log_means], [log_stds]]",
                    ));
                }
                VectorDistribution::LogNormal {
                    log_mean: params[0].clone(),
                    log_std: params[1].clone(),
                }
            }
            "exponential" => {
                if params.is_empty() {
                    return Err(PyValueError::new_err("Exponential requires [[rates]]"));
                }
                VectorDistribution::Exponential {
                    rate: params[0].clone(),
                }
            }
            "uniform" => {
                if params.len() < 2 {
                    return Err(PyValueError::new_err("Uniform requires [[lows], [highs]]"));
                }
                VectorDistribution::Uniform {
                    low: params[0].clone(),
                    high: params[1].clone(),
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

/// Writer for a `Number` source. Compares a value vector against every
/// registered threshold element-wise: 1.0 where the comparison holds, else 0.0.
#[pyclass(name = "NumberWriter")]
struct PyNumberWriter {
    writer: IpcNumberIntervalWriter,
}

#[pymethods]
impl PyNumberWriter {
    /// Sends `value`, one number per value slot (length `value_size`).
    /// `timestamp` is in seconds; `None` uses the current Unix time.
    #[pyo3(signature = (value, timestamp=None))]
    pub fn write(&self, _py: Python<'_>, value: PyArrayLike1<'_, f64>, timestamp: Option<f64>) {
        self.writer.write(array_like_to_vector(value), timestamp);
    }
}

/// Writer for a `Boolean` source. Maps a Python bool to a probability:
/// `True` → 1.0, `False` → 0.0.
#[pyclass(name = "BooleanWriter")]
struct PyBooleanWriter {
    writer: IpcBooleanWriter,
}

#[pymethods]
impl PyBooleanWriter {
    /// Sends `value` to all value slots.
    /// `timestamp` is in seconds; `None` uses the current Unix time.
    #[pyo3(signature = (value, timestamp=None))]
    pub fn write(&self, _py: Python<'_>, value: bool, timestamp: Option<f64>) {
        self.writer.write(value, timestamp);
    }
}

/// Writer for a `Categorical` source. Sends a flat probability matrix
/// `[col₀, col₁, …]` where each column has `value_size` entries, one per
/// value slot.
#[pyclass(name = "CategoricalWriter")]
struct PyCategoricalWriter {
    writer: IpcCategoricalWriter,
}

#[pymethods]
impl PyCategoricalWriter {
    /// Sends class probabilities as a flat list of length
    /// `n_categories() * value_size()`, category by category.
    /// `timestamp` is in seconds; `None` uses the current Unix time.
    #[pyo3(signature = (probabilities, timestamp=None))]
    pub fn write(
        &self,
        _py: Python<'_>,
        probabilities: PyArrayLike1<'_, f64>,
        timestamp: Option<f64>,
    ) {
        self.writer
            .write(array_like_to_vector(probabilities), timestamp);
    }

    /// Number of categories of the source.
    pub fn n_categories(&self) -> usize {
        self.writer.n_categories()
    }

    /// Number of value slots per category.
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

/// A compiled Resin program: its sources, reactive circuit and writers.
///
/// Create it with `Resin.compile(model)`, feed source values through writers
/// from `make_writer`, and read target values from `get_reactive_circuit()`.
#[pyclass(name = "Resin")]
struct PyResin {
    resin: Arc<Mutex<ResinVariant>>,
}

#[pymethods]
impl PyResin {
    /// Compiles a Resin program into a runtime instance. Every declared target
    /// is solved with Clingo and added to one shared reactive circuit.
    ///
    /// - `model`: the Resin program text.
    /// - `value_size`: number of values (probabilities) per source, e.g.
    ///   particles or grid cells evaluated in parallel. `ProbGradient` only
    ///   supports `1`.
    /// - `verbose`: print the generated ASP programs and model counts.
    /// - `semiring` (case-insensitive): `"LogProb"` (default), `"MaxProduct"`,
    ///   `"Fuzzy"`, `"Boolean"`, `"ProbGradient"` or `"CriticalExplanation"`.
    /// - `update_threshold`: minimum change of a leaf value that triggers
    ///   recomputation.
    /// - `max_models`: raise an error if a target has more stable models.
    ///
    /// Raises `RuntimeError` if the program cannot be compiled.
    #[staticmethod]
    #[pyo3(signature = (model, value_size=1, verbose=false, semiring=None, update_threshold=1e-3, max_models=None))]
    fn compile(
        py: Python<'_>,
        model: &str,
        value_size: usize,
        verbose: bool,
        semiring: Option<&str>,
        update_threshold: f64,
        max_models: Option<usize>,
    ) -> PyResult<Self> {
        let model = model.to_string();
        let semiring = semiring.unwrap_or("LogProb").to_string();
        let variant = py
            .detach(move || -> Result<ResinVariant, String> {
                macro_rules! compile_as {
                    ($s:ident) => {
                        Resin::<$s>::compile(
                            &model,
                            value_size,
                            update_threshold,
                            verbose,
                            max_models,
                        )
                        .map(ResinVariant::$s)
                        .map_err(|e| e.to_string())
                    };
                }
                with_semiring!(semiring, compile_as)
            })
            .map_err(PyRuntimeError::new_err)?;
        Ok(PyResin {
            resin: Arc::new(Mutex::new(variant)),
        })
    }

    /// Returns the reactive circuit of this program, which computes the
    /// targets (`update`, `full_update`) and adapts to source frequencies.
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
            ResinVariant::CriticalExplanation(r) => {
                RCVariant::CriticalExplanation(r.manager.reactive_circuit.clone())
            }
        };
        PyReactiveCircuit { circuit }
    }

    /// Connects leaf `receiver_idx` to IPC `channel`: values sent there update
    /// the leaf, as `1 - value` if `invert`. Sources are connected
    /// automatically; this is only needed for custom wiring.
    /// Raises `IOError` if the channel cannot be opened.
    fn read(&self, py: Python<'_>, receiver_idx: u32, channel: &str, invert: bool) -> PyResult<()> {
        let channel = channel.to_string();
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.read(receiver_idx, &channel, invert).map_err(|e| e.to_string()))
        })
        .map_err(PyIOError::new_err)
    }

    /// Returns the writer for the source declared on `channel`, e.g.
    /// `make_writer("/sensors/speed")`. Its type follows the source type:
    /// `ProbabilityWriter`, `DensityWriter`, `NumberWriter` or `BooleanWriter`.
    /// For categorical sources, use `make_categorical_writer`.
    /// Raises `RuntimeError` if no such source uses `channel`.
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

    /// Like `make_writer`, but looks the source up by its atom name, e.g.
    /// `make_writer_for("speed")` or `make_writer_for("distance(hospital)")`.
    /// Raises `RuntimeError` if no source has that name.
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

    /// Returns the `CategoricalWriter` for the categorical source on `channel`.
    /// Raises `RuntimeError` if no categorical source uses `channel`.
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

    /// Starts a background writer that sends a value to `channel` at
    /// `frequency` Hz, e.g. to simulate a sensor. Returns the `SharedVector`
    /// holding the value it sends; change it with `set`.
    /// Raises `IOError` if the writer cannot be created.
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

    /// Stops all writers started with `make_timed_writer`.
    fn stop_timed_writers(&self, py: Python<'_>) {
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.stop_timed_writers())
        })
    }

    /// Names of all circuit leaves in index order, e.g. `"alarm"` and its
    /// complement `"-alarm"`. The position of a name is the leaf index used
    /// by `ReactiveCircuit.lift_leaf` and `drop_leaf`.
    fn get_names(&self, py: Python<'_>) -> Vec<String> {
        let resin = self.resin.clone();
        py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.get_names())
        })
    }

    /// Estimated update frequency (Hz) of every leaf, in index order.
    fn get_frequencies<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<f64>> {
        let resin = self.resin.clone();
        let frequencies = py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.get_frequencies())
        });
        PyArray1::from_vec(py, frequencies)
    }

    /// Current value of every leaf as an array of length `value_size`, in
    /// index order.
    fn get_values<'py>(&self, py: Python<'py>) -> Vec<Bound<'py, PyArray1<f64>>> {
        let resin = self.resin.clone();
        let values = py.detach(move || {
            let mut guard = resin.lock().unwrap();
            with_resin!(guard, r => r.manager.get_values())
        });
        values.iter().map(|v| vector_to_pyarray(py, v)).collect()
    }

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

    /// Returns the learnable parameters found during compilation.
    ///
    /// Each key is `"{predicate}#{clause_index}"`, e.g. `"needs_checkup#0"`,
    /// and maps to the names of the leaves sharing that clause's `P(...)` value.
    /// The keys are what `fit_parameters` accepts as `parameters`.
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
                ResinVariant::CriticalExplanation(r) => r.get_parameter_groups().clone(),
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
    /// - `gradients`: the `"gradients"` dict of one target from
    ///   `ReactiveCircuit.gradient_update()`.
    /// - `lr`: learning rate.
    /// - `loss`: derivative of the loss with respect to the target
    ///   probability, e.g. `2 * (p - label)` for squared error.
    /// - `parameters`: keys from `get_parameter_groups()` to update, or `None`
    ///   for all.
    /// - `timestamp`: time of the update in seconds.
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

/// The reactive circuit computing a program's targets. Obtain it with
/// `Resin.get_reactive_circuit()`, or build one by hand from leaves and
/// sum-products.
///
/// Leaves are addressed by index (see `Resin.get_names()`). Writing to a
/// source marks the dependent circuit nodes as outdated; `update()` then
/// recomputes only those, `full_update()` everything.
#[pyclass(name = "ReactiveCircuit")]
struct PyReactiveCircuit {
    circuit: RCVariant,
}

#[pymethods]
impl PyReactiveCircuit {
    /// Creates an empty circuit with `value_size` values per leaf.
    ///
    /// `semiring` and `value_size` are chosen as in `Resin.compile` (default
    /// `"LogProb"`).
    /// Leaf changes below `update_threshold` do not trigger recomputation.
    /// Raises `ValueError` for an unknown semiring or invalid `value_size`.
    #[new]
    #[pyo3(signature = (value_size, update_threshold=1e-3, semiring=None))]
    fn new(value_size: usize, update_threshold: f64, semiring: Option<&str>) -> PyResult<Self> {
        macro_rules! new_circuit {
            ($s:ident) => {
                $s::validate_value_size(value_size).map(|()| {
                    let mut rc = ReactiveCircuit::<$s>::new($s::circuit_value_size(value_size));
                    rc.update_threshold = update_threshold;
                    if let Some(size) = $s::auto_value_size(0) {
                        rc.set_value_size(size);
                    }
                    RCVariant::$s(Arc::new(Mutex::new(rc)))
                })
            };
        }
        let circuit = with_semiring!(semiring.unwrap_or("LogProb"), new_circuit)
            .map_err(PyValueError::new_err)?;
        Ok(PyReactiveCircuit { circuit })
    }

    /// Adds a leaf with an initial value (`value_size` probabilities), an
    /// initial update frequency in Hz and a name. Returns its leaf index.
    fn add_leaf(
        &self,
        py: Python<'_>,
        initial_value: PyArrayLike1<'_, f64>,
        frequency: f64,
        token: String,
    ) -> PyResult<usize> {
        let initial_value = array_like_to_vector(initial_value);
        let circuit = self.circuit.clone();
        Ok(py.detach(
            move || with_rc!(circuit, c => c.add_leaf(initial_value, frequency, &token) as usize),
        ))
    }

    /// Sets the value of leaf `leaf_index` (`value_size` probabilities) at
    /// `timestamp` (seconds), updates its frequency estimate and marks
    /// dependent nodes as outdated if the value changed by more than the
    /// update threshold.
    fn update_leaf(
        &self,
        py: Python<'_>,
        leaf_index: u32,
        new_value: PyArrayLike1<'_, f64>,
        timestamp: f64,
    ) -> PyResult<()> {
        let new_value = array_like_to_vector(new_value);
        let circuit = self.circuit.clone();
        py.detach(
            move || with_rc!(circuit, c => leaf::update(&mut c, leaf_index, new_value, timestamp)),
        );
        Ok(())
    }

    /// Adds a sum of products of leaf indices to target `target_token`, e.g.
    /// `[[0, 1], [2]]` for `x0·x1 + x2`. Creates the target if needed.
    fn add_sum_product(&self, py: Python<'_>, sum_product: Vec<Vec<u32>>, target_token: &str) {
        let target_token = target_token.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.add_sum_product(&sum_product, &target_token)))
    }

    /// Restructures the circuit by leaf update frequency: leaves are grouped
    /// into `number_bins` bands of `bin_size` Hz, and faster leaves are placed
    /// higher, so frequent updates recompute only small parts of the circuit.
    ///
    /// `dag=True` (default) shares identical sub-circuits between targets;
    /// `dag=False` keeps a separate tree per target.
    #[pyo3(signature = (bin_size, number_bins, dag=true))]
    fn adapt(&self, py: Python<'_>, bin_size: f64, number_bins: usize, dag: bool) {
        let circuit = self.circuit.clone();
        let topology = topology(dag);
        py.detach(move || {
            let boundaries = crate::channels::clustering::create_boundaries(bin_size, number_bins);
            with_rc!(circuit, c => c.adapt(&boundaries, topology))
        })
    }

    /// Recomputes the outdated parts of the circuit. Returns a dict from target
    /// channel to its value array, containing only the targets that were
    /// recomputed. Each array has `value_size` values; for
    /// `CriticalExplanation`, it holds the four blocks `[P | v | tag | w]` of
    /// `value_size` values each.
    fn update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let circuit = self.circuit.clone();
        let results = py.detach(move || with_rc!(circuit, c => c.update()));
        let dict = PyDict::new(py);
        for (token, vector) in &results {
            dict.set_item(token, vector_to_pyarray(py, vector))?;
        }
        Ok(dict.into())
    }

    /// Recomputes the whole circuit and returns the values of all targets,
    /// in the same format as `update()`.
    fn full_update(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let circuit = self.circuit.clone();
        let results = py.detach(move || with_rc!(circuit, c => c.full_update()));
        let dict = PyDict::new(py);
        for (token, vector) in &results {
            dict.set_item(token, vector_to_pyarray(py, vector))?;
        }
        Ok(dict.into())
    }

    /// Like `update()`, but returns `{target: {"probability": p, "gradients":
    /// {leaf_name: dp/dleaf}}}` for the recomputed targets.
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

    /// Like `full_update()`, in the format of `gradient_update()`.
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

    /// One gradient-descent step on leaf probabilities:
    /// `p ← clamp(p - lr * loss * gradient, 0, 1)`.
    ///
    /// `gradients` is the `"gradients"` dict of one target from
    /// `gradient_update()`, `loss` the derivative of the loss with respect to
    /// the target probability, `atoms` the leaf names to update (`None` for
    /// all) and `timestamp` the time of the update in seconds. To learn
    /// `P(...)` clause parameters of a program, use `Resin.fit_parameters`.
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

    /// Moves leaf `index` one level up, into the parents of the nodes that
    /// contain it. Use this for a leaf that changes more often than others.
    /// `dag` as in `adapt`.
    #[pyo3(signature = (index, dag=true))]
    fn lift_leaf(&self, py: Python<'_>, index: u32, dag: bool) {
        let circuit = self.circuit.clone();
        let topology = topology(dag);
        py.detach(move || with_rc!(circuit, c => c.lift_leaf(index, topology)))
    }

    /// Moves leaf `index` one level down, into the children of the nodes that
    /// contain it. Use this for a leaf that changes less often than others.
    /// `dag` as in `adapt`.
    #[pyo3(signature = (index, dag=true))]
    fn drop_leaf(&self, py: Python<'_>, index: u32, dag: bool) {
        let circuit = self.circuit.clone();
        let topology = topology(dag);
        py.detach(move || with_rc!(circuit, c => c.drop_leaf(index, topology)))
    }

    /// Writes the circuit structure as a Graphviz dot file to `path`.
    fn to_dot(&self, py: Python<'_>, path: &str) -> PyResult<()> {
        let path = path.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.to_dot(&path)))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    /// Renders the circuit structure to an SVG at `path` (requires Graphviz);
    /// with `keep_dot`, the dot source is kept as `path + ".dot"`.
    #[pyo3(signature = (path, keep_dot=false))]
    fn to_svg(&self, py: Python<'_>, path: &str, keep_dot: bool) -> PyResult<()> {
        let path = path.to_string();
        let circuit = self.circuit.clone();
        py.detach(move || with_rc!(circuit, c => c.to_svg(&path, keep_dot)))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    /// Renders the circuit together with the formula inside every node to an
    /// SVG at `path` (requires Graphviz).
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

/// Resin: reactive probabilistic logic programming with Reactive Circuits.
///
/// A Resin program declares sources (incoming signals), rules and targets.
/// `Resin.compile` turns it into a reactive circuit that recomputes only what
/// changed when sources are written.
///
/// Example:
///
/// ```python
/// from resin import Resin
///
/// resin = Resin.compile('''
///     rain <- source("/weather/rain", Probability).
///     wet if rain.
///     wet -> target("/wet").
/// ''')
/// resin.make_writer("/weather/rain").write([0.3])
/// rc = resin.get_reactive_circuit()
/// print(rc.update())  # {'/wet': array([0.3])}
/// ```
///
/// Main classes: `Resin` (compile programs, create writers), `ReactiveCircuit`
/// (update targets, adapt to source frequencies) and one writer class per
/// source type.
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
