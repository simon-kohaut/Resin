use crate::tracking::Kalman;
use crate::tracking::LinearModel;
use ndarray::array;
use std::time::SystemTime;
use std::time::SystemTimeError;

pub struct FoCEstimator {
    kalman: Kalman,
    clock: SystemTime,
}

impl FoCEstimator {
    pub fn new(frequency: &f64) -> Self {
        let forward_model = array![[1.0, 1.0], [0.0, 1.0]];
        let input_model = array![[0.0, 0.0]];
        let output_model = array![[1.0, 0.0]];
        let prediction = array![frequency.to_owned(), 0.0];
        let prediction_covariance = array![[1.0, 0.0], [0.0, 1.0]];
        let process_noise = array![[1.0, 0.0], [0.0, 1.0]];
        let sensor_noise = array![[10.0]];

        let model = LinearModel::new(&forward_model, &input_model, &output_model);
        Self {
            kalman: Kalman::new(
                &prediction,
                &prediction_covariance,
                &process_noise,
                &sensor_noise,
                &model,
            ),
            clock: SystemTime::now(),
        }
    }

    pub fn update(&mut self) -> Result<f64, SystemTimeError> {
        match self.clock.elapsed() {
            Ok(elapsed) => {
                self.clock = SystemTime::now();
                self.kalman.predict(None);
                self.kalman.update(&array![elapsed.as_secs_f64()]);
                return Ok(1.0 / self.kalman.estimate[0]);
            }
            Err(e) => return Err(e),
        }
    }
}
