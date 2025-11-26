use nalgebra::{DMatrix, linalg::try_invert_to};

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
            kalman_gain: Matrix::zeros((x_dim, z_dim)),
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

        // Invert the residual covariance with nalgebra
        let mut inverse = DMatrix::zeros(self.residual_covariance.nrows(), self.residual_covariance.ncols()); 
        let nalbebra_covaraince =         DMatrix::from_row_slice(
            self.residual_covariance.nrows(),
            self.residual_covariance.ncols(),
            self.residual_covariance.as_slice().unwrap(),
        );
        try_invert_to(nalbebra_covaraince, &mut inverse);
        let inverted_covariance = Matrix::from_shape_vec(
            (self.residual_covariance.nrows(), self.residual_covariance.ncols()),
            inverse.as_slice().to_vec(),
        ).expect("Failed to invert Kalman residual covariance matrix");
         
        // Compute the new Kalman gain
        self.kalman_gain = self
            .prediction_covariance
            .dot(&self.model.output_model.t())
            .dot(&inverted_covariance);

        // Estimate new state
        self.estimate = &self.prediction + &self.kalman_gain.dot(&self.residual);
        self.estimate_covariance = &self.prediction_covariance
            - &self
                .kalman_gain
                .dot(&self.model.output_model)
                .dot(&self.prediction_covariance);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::{array, Array2};

    #[test]
    fn test_kalman_new() {
        let forward_model = |dt| array![[1.0, dt], [0.0, 1.0]];
        let input_model = array![[0.0], [0.0]];
        let output_model = array![[1.0, 0.0]];
        let prediction = array![0.0, 0.0];
        let prediction_covariance = array![[1.0, 0.0], [0.0, 1.0]];
        let process_noise = array![[0.1, 0.0], [0.0, 0.1]];
        let sensor_noise = array![[0.5]];

        let model = LinearModel::new(forward_model, &input_model, &output_model);
        let kalman = Kalman::new(
            &prediction,
            &prediction_covariance,
            &process_noise,
            &sensor_noise,
            &model,
        );

        assert_eq!(kalman.prediction, prediction);
        assert_eq!(kalman.prediction_covariance, prediction_covariance);
        assert_eq!(kalman.estimate, array![0.0, 0.0]);
        assert_eq!(kalman.estimate_covariance, array![[0.0, 0.0], [0.0, 0.0]]);
        assert_eq!(kalman.process_noise, process_noise);
        assert_eq!(kalman.sensor_noise, sensor_noise);
    }

    #[test]
    fn test_kalman_constant_stream() {
        // 1D Kalman filter for a constant value.
        let forward_model = |_dt: f64| array![[1.0]];
        let input_model = array![[0.0]];
        let output_model = array![[1.0]];
        let model = LinearModel::new(forward_model, &input_model, &output_model);

        // Initial state is 0, with some uncertainty.
        let prediction = array![0.0];
        let prediction_covariance = array![[1.0]];

        // Low process and sensor noise since we send correct, constant data
        let process_noise = array![[0.1]];
        let sensor_noise = array![[0.1]];

        let mut kalman = Kalman::new(
            &prediction,
            &prediction_covariance,
            &process_noise,
            &sensor_noise,
            &model,
        );

        let constant_value = 10.0;
        let measurement = array![constant_value];

        // Run the filter for a few iterations to let it converge.
        for _ in 0..100 {
            kalman.predict(1.0, None); // dt=1.0, no input
            kalman.update(&measurement);
        }

        // The estimate should be very close to the constant measurement.
        assert!((kalman.estimate[0] - constant_value).abs() < 1e-3);
    }
}