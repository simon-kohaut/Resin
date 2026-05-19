use crate::tracking::Kalman;
use crate::tracking::LinearModel;
use ndarray::array;

/// Frequency-of-Change (FoC) estimator for a single leaf.
///
/// Tracks the time between successive `update` calls and uses a Kalman filter
/// to produce a smoothed estimate of the leaf's update frequency in Hz.
/// The filter models frequency as a constant with additive noise, using a
/// constant-velocity forward model.
#[derive(Clone, Debug)]
pub struct FoCEstimator {
    pub kalman: Kalman,
    pub timestamp: Option<f64>,
}

impl FoCEstimator {
    /// Creates a new estimator seeded with `frequency` as the initial estimate.
    /// The Kalman model is a 2-state (inter-arrival time, drift) constant-velocity
    /// filter observed through inter-arrival time measurements.
    pub fn new(frequency: f64) -> Self {
        let forward_model = |dt| array![[1.0, dt], [0.0, 1.0]];
        let input_model = array![[0.0, 0.0]];
        let output_model = array![[1.0, 0.0]];
        let estimate = array![frequency, 0.0];
        let estimate_covariance = array![[30.0, 0.0], [0.0, 100.0]];
        let process_noise = array![[0.05, 1.0], [1.0, 20.0]];
        let sensor_noise = array![[0.05]];

        let model = LinearModel::new(forward_model, &input_model, &output_model);
        Self {
            kalman: Kalman::new(
                &estimate,
                &estimate_covariance,
                &process_noise,
                &sensor_noise,
                &model,
            ),
            timestamp: None,
        }
    }

    /// Resets the Kalman filter to a zero-frequency estimate with default
    /// initial covariance, and clears the last-seen timestamp.
    pub fn reset(&mut self) {
        let estimate = array![0.0, 0.0];
        let estimate_covariance = array![[30.0, 0.0], [0.0, 100.0]];

        self.kalman.reset(&estimate, &estimate_covariance);
    }

    /// Records a new observation at `timestamp` (Unix seconds), runs a
    /// predict-correct Kalman cycle using the elapsed time since the last call,
    /// and returns the updated frequency estimate in Hz.
    ///
    /// Returns `0.0` on the very first call (no elapsed time available yet).
    /// The internal estimate is clamped to `[0.0001, 100.0]` Hz.
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

    /// Like `update`, but takes the elapsed time directly instead of a wall
    /// timestamp.  Useful for simulation or replay.
    /// The internal estimate is clamped to `[0.0001, 1000.0]` Hz.
    pub fn update_elapsed(&mut self, elapsed: f64) -> f64 {
        self.kalman.predict(elapsed, None);
        self.kalman.update(&array![elapsed]);
        self.kalman.estimate[0] = self.kalman.estimate[0].clamp(0.0001, 1000.0);

        1.0 / self.kalman.estimate[0]
    }
}
