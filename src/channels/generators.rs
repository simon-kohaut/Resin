use rand_distr::{Distribution, Normal, SkewNormal, Uniform};

pub fn generate_uniform_frequencies(low: f64, high: f64, number_samples: usize) -> Vec<f64> {
    let distribution = Uniform::new(low, high);
    let mut rng = rand::thread_rng();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rng));
    }

    frequencies
}

pub fn generate_normal_frequencies(location: f64, scale: f64, number_samples: usize) -> Vec<f64> {
    let distribution = Normal::new(location, scale).unwrap();
    let mut rng = rand::thread_rng();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rng));
    }

    frequencies
}

pub fn generate_skew_normal_frequencies(
    location: f64,
    scale: f64,
    shape: f64,
    number_samples: usize,
) -> Vec<f64> {
    let distribution = SkewNormal::new(location, scale, shape).unwrap();
    let mut rng = rand::thread_rng();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rng));
    }

    frequencies
}
