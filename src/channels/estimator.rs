use crate::tracking::Kalman;
use crate::tracking::LinearModel;
use ndarray::array;

#[derive(Clone)]
pub struct FoCEstimator {
    pub kalman: Kalman,
    pub timestamp: Option<f64>,
}

impl FoCEstimator {
    pub fn new(frequency: f64) -> Self {
        let forward_model = |dt| array![[1.0, dt], [0.0, 1.0]];
        let input_model = array![[0.0, 0.0]];
        let output_model = array![[1.0, 0.0]];
        let prediction = array![frequency, 0.0];
        let prediction_covariance = array![[30.0, 0.0], [0.0, 100.0]];
        let process_noise = array![[0.05, 1.0], [1.0, 20.0]];
        let sensor_noise = array![[0.05]];

        let model = LinearModel::new(forward_model, &input_model, &output_model);
        Self {
            kalman: Kalman::new(
                &prediction,
                &prediction_covariance,
                &process_noise,
                &sensor_noise,
                &model,
            ),
            timestamp: None,
        }
    }

    pub fn update(&mut self, timestamp: f64) -> f64 {
        // Very first update, do not estimate
        if self.timestamp.is_none() {
            self.timestamp = Some(timestamp);
            return 0.0;
        }

        // Get elapsed time since last call and set new timestamp
        let elapsed = timestamp - self.timestamp.unwrap();
        self.timestamp = Some(timestamp);

        // Predict-correct cycle
        self.kalman.predict(elapsed, None);
        self.kalman.update(&array![elapsed]);

        // Ensure that we never estimate a negative time between updates
        self.kalman.estimate[0] = self.kalman.estimate[0].clamp(0.0001, 100.0);

        // Return frequency as inverse of estimated time delta
        1.0 / self.kalman.estimate[0]
    }

    pub fn update_elapsed(&mut self, elapsed: f64) -> f64 {
        self.kalman.predict(elapsed, None);
        self.kalman.update(&array![elapsed]);
        self.kalman.estimate[0] = self.kalman.estimate[0].clamp(0.0001, 1000.0);

        1.0 / self.kalman.estimate[0]
    }
}

#[cfg(test)]
mod tests {

    use std::{
        fs::{File, OpenOptions},
        io::Write,
        path::Path,
    };

    use rand_distr::{Distribution, Normal};

    use super::FoCEstimator;
    use crate::channels::clustering::{binning, create_boundaries};
    use crate::channels::generators::generate_uniform_frequencies;

    #[test]
    fn test_foc_estimation() {
        // Test settings
        let low = 0.0;
        let high = 30.0;
        let number_samples = 1000;
        let number_measurements = 20;
        let number_repetitions = 60;
        let bin_sizes = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

        // Create estimators
        let mut estimators = vec![];
        for _ in 0..number_samples {
            estimators.push(FoCEstimator::new(0.0));
        }

        // Prepare test log to be written
        let path = Path::new("output/data/foc_estimation.csv");
        if !path.exists() {
            let mut file = File::create(path).expect("Unable to create file");
            file.write_all(
                "Estimator,Measurement,True,Estimated,BinSize,TrueCluster,EstimatedCluster\n"
                    .as_bytes(),
            )
            .expect("Unable to write data");
        }

        // Append to CSV
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        let mut csv_text = "".to_string();

        // Start estimation process
        let mut rng = rand::rng();
        for (i, estimator) in &mut estimators.iter_mut().enumerate() {
            for bin_size in bin_sizes {
                let boundaries = create_boundaries(bin_size, 100);

                // Sample new random frequencies and clusters
                let mut true_frequency = generate_uniform_frequencies(low, high, 1)[0];
                let mut true_cluster = binning(&vec![true_frequency], &boundaries)[0];

                // Write down initial values
                let initial = estimator.kalman.prediction[0];
                let initial_cluster = binning(&vec![initial], &boundaries)[0];
                csv_text.push_str(&format!("{i},0,{true_frequency},{initial},{bin_size},{true_cluster},{initial_cluster}\n"));

                for k in 0..number_repetitions {
                    for j in k * number_measurements + 1..=(k + 1) * number_measurements {
                        let noisy_elapsed =
                            1.0 / Normal::new(true_frequency, 0.25).unwrap().sample(&mut rng);

                        let estimated = estimator.update_elapsed(noisy_elapsed).clamp(0.0, 100.0);
                        let estimated_cluster = binning(&vec![estimated], &boundaries)[0];

                        csv_text.push_str(&format!("{i},{j},{true_frequency},{estimated},{bin_size},{true_cluster},{estimated_cluster}\n"));
                    }

                    // Sample new random frequencies and clusters
                    true_frequency = generate_uniform_frequencies(low, high, 1)[0];
                    true_cluster = binning(&vec![true_frequency], &boundaries)[0];
                }
            }
        }

        // Write to file
        file.write_all(csv_text.as_bytes())
            .expect("Unable to write data");
    }
}
