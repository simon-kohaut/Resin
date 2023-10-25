use crate::tracking::Kalman;
use crate::tracking::LinearModel;
use ndarray::array;
use std::time::Instant;

#[derive(Clone)]
pub struct FoCEstimator {
    kalman: Kalman,
    clock: Instant,
}

impl FoCEstimator {
    pub fn new(frequency: &f64) -> Self {
        let forward_model = array![[1.0, 1.0], [0.0, 0.0]];
        let input_model = array![[0.0, 0.0]];
        let output_model = array![[1.0, 0.0]];
        let prediction = array![frequency.to_owned(), 0.0];
        let prediction_covariance = array![[30.0, 0.0], [0.0, 30.0]];
        let process_noise = array![[0.1, 0.0], [0.0, 0.1]];
        let sensor_noise = array![[20.0]];

        let model = LinearModel::new(&forward_model, &input_model, &output_model);
        Self {
            kalman: Kalman::new(
                &prediction,
                &prediction_covariance,
                &process_noise,
                &sensor_noise,
                &model,
            ),
            clock: Instant::now(),
        }
    }

    pub fn update(&mut self) -> f64 {
        self.kalman.predict(None);
        self.kalman
            .update(&array![self.clock.elapsed().as_secs_f64()]);
        self.clock = Instant::now();

        1.0 / self.kalman.estimate[0]
    }
}
