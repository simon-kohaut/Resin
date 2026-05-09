use rand_distr::{Distribution, Normal, SkewNormal, Uniform};

/// Draws `number_samples` values uniformly from `[low, high)`.
pub fn generate_uniform_frequencies(low: f64, high: f64, number_samples: usize) -> Vec<f64> {
    let distribution = Uniform::new(low, high).unwrap();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rand::rng()));
    }

    frequencies
}

/// Draws `number_samples` values from a Normal distribution with the given
/// `location` (mean) and `scale` (standard deviation).
pub fn generate_normal_frequencies(location: f64, scale: f64, number_samples: usize) -> Vec<f64> {
    let distribution = Normal::new(location, scale).unwrap();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rand::rng()));
    }

    frequencies
}

/// Draws `number_samples` values from a Skew-Normal distribution with the
/// given `location`, `scale`, and `shape` (skewness) parameters.
pub fn generate_skew_normal_frequencies(
    location: f64,
    scale: f64,
    shape: f64,
    number_samples: usize,
) -> Vec<f64> {
    let distribution = SkewNormal::new(location, scale, shape).unwrap();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rand::rng()));
    }

    frequencies
}
