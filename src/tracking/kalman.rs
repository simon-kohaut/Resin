use ndarray_linalg::Inverse;

use super::{LinearModel, Matrix, Vector};

#[derive(Clone, Debug)]
pub struct Kalman {
    // Gaussian estimation of state
    pub prediction: Vector,
    pub prediction_covariance: Matrix,
    pub estimate: Vector,
    pub estimate_covariance: Matrix,

    // The model of the tracked process
    model: LinearModel,

    // Noise as covariance matrices
    process_noise: Matrix,
    sensor_noise: Matrix,

    // Kalman values
    residual: Vector,
    residual_covariance: Matrix,
    kalman_gain: Matrix,
}

impl Kalman {
    pub fn new(
        estimate: &Vector,
        estimate_covariance: &Matrix,
        process_noise: &Matrix,
        sensor_noise: &Matrix,
        model: &LinearModel,
    ) -> Self {
        let x_dim = model.get_state_dimension();
        let z_dim = model.get_measurement_dimension();

        Self {
            prediction: Vector::zeros(x_dim),
            prediction_covariance: Matrix::zeros((x_dim, x_dim)),
            estimate: estimate.clone(),
            estimate_covariance: estimate_covariance.clone(),
            model: model.clone(),
            process_noise: process_noise.clone(),
            sensor_noise: sensor_noise.clone(),
            residual: Vector::zeros(z_dim),
            residual_covariance: Matrix::zeros((z_dim, z_dim)),
            kalman_gain: Matrix::zeros((z_dim, x_dim)),
        }
    }

    pub fn reset(&mut self, estimate: &Vector, estimate_covariance: &Matrix) {
        self.estimate = estimate.clone();
        self.estimate_covariance = estimate_covariance.clone();

        let x_dim = self.model.get_state_dimension();
        self.prediction = Vector::zeros(x_dim);
        self.prediction_covariance = Matrix::zeros((x_dim, x_dim));
    }

    pub fn predict(&mut self, dt: f64, input: Option<&Vector>) {
        // Predict next state and prediction covariance
        self.prediction = self.model.forward(&self.estimate, dt, input);
        self.prediction_covariance = (self.model.forward_model)(dt)
            .dot(&self.estimate_covariance)
            .dot(&(self.model.forward_model)(dt).t())
            + &self.process_noise;
    }

    pub fn update(&mut self, measurement: &Vector) {
        // Compute the residual and its covariance
        self.residual = measurement - &self.model.measure(&self.prediction);
        self.residual_covariance = self
            .model
            .output_model
            .dot(&self.prediction_covariance)
            .dot(&self.model.output_model.t())
            + &self.sensor_noise;

        // Compute the new Kalman gain
        self.kalman_gain = self
            .prediction_covariance
            .dot(&self.model.output_model.t())
            .dot(&self.residual_covariance.inv().unwrap());

        // Estimate new state
        self.estimate = &self.prediction + &self.kalman_gain.dot(&self.residual);
        self.estimate_covariance = &self.prediction_covariance
            - &self
                .kalman_gain
                .dot(&self.model.output_model)
                .dot(&self.prediction_covariance);
    }
}
