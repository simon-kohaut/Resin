use super::{Matrix, Vector};

/// Describes the dynamics of a linear time-varying system for use with the
/// Kalman filter.
///
/// - `forward_model(dt)` — the state-transition matrix `F(dt)`.
/// - `input_model` — the control-input matrix `B`.
/// - `output_model` — the measurement matrix `H` (maps state → measurement).
#[derive(Clone, Debug)]
pub struct LinearModel {
    pub forward_model: fn(f64) -> Matrix,
    pub input_model: Matrix,
    pub output_model: Matrix,
}

impl LinearModel {
    /// Creates a `LinearModel` from a state-transition function and the
    /// input/output matrices.
    pub fn new(
        forward_model: fn(f64) -> Matrix,
        input_model: &Matrix,
        output_model: &Matrix,
    ) -> Self {
        Self {
            forward_model,
            input_model: input_model.clone(),
            output_model: output_model.clone(),
        }
    }

    /// Returns the number of state dimensions (rows of `F`).
    pub fn get_state_dimension(&self) -> usize {
        (self.forward_model)(0.0).shape()[0]
    }

    /// Returns the number of measurement dimensions (rows of `H`).
    pub fn get_measurement_dimension(&self) -> usize {
        self.output_model.shape()[0]
    }

    /// Applies the forward model: `F(dt) * state + B * input` (or just
    /// `F(dt) * state` when `input` is `None`).
    pub fn forward(&self, state: &Vector, dt: f64, input: Option<&Vector>) -> Vector {
        match input {
            Some(vector) => (self.forward_model)(dt).dot(state) + self.input_model.dot(vector),
            None => (self.forward_model)(dt).dot(state),
        }
    }

    /// Applies the output model: `H * state`.
    pub fn measure(&self, state: &Vector) -> Vector {
        self.output_model.dot(state)
    }
}
