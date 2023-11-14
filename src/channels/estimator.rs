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
        let forward_model = |dt| array![[1.0, dt], [0.0, 1.0]];
        let input_model = array![[0.0, 0.0]];
        let output_model = array![[1.0, 0.0]];
        let prediction = array![frequency.to_owned(), 0.0];
        let prediction_covariance = array![[30.0, 0.0], [0.0, 100.0]];
        let process_noise = array![[0.05, 1.0], [1.0, 20.0]];
        let sensor_noise = array![[0.01]];

        let model = LinearModel::new(forward_model, &input_model, &output_model);
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
        let elapsed = self.clock.elapsed().as_secs_f64();

        self.kalman.predict(elapsed, None);
        self.kalman.update(&array![elapsed]);
        self.clock = Instant::now();

        1.0 / self.kalman.estimate[0]
    }

    pub fn update_elapsed(&mut self, elapsed: f64) -> f64 {
        self.kalman.predict(elapsed, None);
        self.kalman.update(&array![elapsed]);
        self.clock = Instant::now();

        1.0 / self.kalman.estimate[0]
    }
}


#[cfg(test)]
mod tests {

    use std::{path::Path, fs::{File, OpenOptions}, io::Write};

    use rand::thread_rng;
    use rand_distr::{Normal, Distribution};

    use crate::channels::generators::generate_uniform_frequencies;
    use crate::channels::clustering::binning;
    use super::FoCEstimator;

    #[test]
    fn test_foc_estimation() {
        // Test settings
        let low = 0.0;
        let high = 30.0;
        let number_samples = 1000;
        let number_measurements = 60;
        let boundaries = vec![2.5, 5.0, 7.5, 10.0, 12.5, 15.0, 17.5, 20.0, 22.5, 25.0, 27.5, 30.0, 32.5, 35.0, 37.5, 40.0, 1000.0];

        // Sample random frequencies
        let true_frequencies = generate_uniform_frequencies(low, high, number_samples);
        
        // Get true cluster assignments
        let true_clusters = binning(&true_frequencies, &boundaries);

        // Create estimators
        let mut estimators = vec![];
        for _ in 0..number_samples {
            estimators.push(FoCEstimator::new(&0.0));
        }

        // Prepare test log to be written
        let path = Path::new("output/data/foc_estimation.csv");
        if !path.exists() {
            let mut file = File::create(path).expect("Unable to create file");
            file.write_all("Measurement,Error,ClusterError,Variance\n".as_bytes())
                .expect("Unable to write data");
        }
        
        // Now append to CSV
        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        let mut csv_text = "".to_string();


        for (i, estimator) in &mut estimators.iter_mut().enumerate() {
            let true_frequency = true_frequencies[i];
            let true_cluster = true_clusters[i];

            let estimated = estimator.kalman.prediction[0];
            let estimated_cluster = binning(&vec![estimated], &boundaries)[0];

            let error = (estimated - true_frequency).abs();
            let cluster_error = true_cluster.abs_diff(estimated_cluster);

            csv_text.push_str(&format!("0,{error},{cluster_error},0\n"));
        }
      
        // Start estimation process
        let mut rng = thread_rng();
        for (i, estimator) in &mut estimators.iter_mut().enumerate() {
            let true_frequency = true_frequencies[i];
            let true_cluster = true_clusters[i];

            for j in 0..number_measurements {
                let noisy_elapsed = 1.0 / Normal::new(true_frequency, 1.0).unwrap().sample(&mut rng);
                let estimated = estimator.update_elapsed(noisy_elapsed).clamp(0.0, 100.0);
                let error = (true_frequency - estimated).abs();

                let estimated_cluster = binning(&vec![estimated], &boundaries)[0];
                let cluster_error = true_cluster.abs_diff(estimated_cluster);

                let variance = estimator.kalman.estimate_covariance[(0, 0)];
                csv_text.push_str(&format!("{},{error},{cluster_error},{variance}\n", j + 1));
            }
        }

        // Shift true frequencies estimation process
        for (i, estimator) in &mut estimators.iter_mut().enumerate() {
            let shifted_i = if i < number_samples - 1 { i + 1 } else { 0 };
            let true_frequency = true_frequencies[shifted_i];
            let true_cluster = true_clusters[shifted_i];

            for j in number_measurements..2 * number_measurements {
                let noisy_elapsed = 1.0 / Normal::new(true_frequency, 1.0).unwrap().sample(&mut rng);
                let estimated = estimator.update_elapsed(noisy_elapsed).clamp(0.0, 100.0);
                let error = (true_frequency - estimated).abs();

                let mut estimated_cluster = 0;
                for (cluster, boundary) in boundaries.iter().enumerate() {
                    if estimated <= *boundary {
                        estimated_cluster = cluster;
                        break;
                    }
                }
                let cluster_error = true_cluster.abs_diff(estimated_cluster);

                let variance = estimator.kalman.estimate_covariance[(0, 0)];
                csv_text.push_str(&format!("{},{error},{cluster_error},{variance}\n", j + 1));
            }
        }

        // Write to file
        file.write_all(csv_text.as_bytes()).expect("Unable to write data");
    }
}
