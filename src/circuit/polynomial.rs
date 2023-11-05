// Standard library
use std::collections::BTreeSet;
use std::ops::{Add, Mul};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

// Third-party
use atomic_float::AtomicF64;

// Crate
use crate::Leaf;

#[derive(Clone)]
pub struct Polynomial {
    pub storage: Arc<AtomicF64>,
    pub valid: Arc<AtomicBool>,
    pub products: Vec<(BTreeSet<u16>, Option<Polynomial>)>,
}

impl Polynomial {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(AtomicF64::new(0.0)),
            valid: Arc::new(AtomicBool::new(false)),
            products: vec![],
        }
    }

    pub fn value(&mut self, foliage: &MutexGuard<Vec<Leaf>>) -> f64 {
        if self.valid.load(Ordering::Acquire) {
            return self.storage.load(Ordering::Acquire);
        }

        let value = self
            .products
            .iter_mut()
            .map(|(factors, sub_polynomial)| {
                // Get product of leafs
                let mut value = factors.iter().fold(1.0, |acc, factor| {
                    acc * foliage[*factor as usize].get_value()
                });

                // Factor in the optional result of polynomial underneath
                if sub_polynomial.is_some() {
                    value *= sub_polynomial.as_mut().unwrap().value(&foliage);
                }

                value
            })
            .sum(); // Sum over all products

        self.valid.store(true, Ordering::Release);
        self.storage.store(value, Ordering::Release);

        value
    }
}

impl Add<Polynomial> for Polynomial {
    type Output = Polynomial;

    fn add(self, rhs: Polynomial) -> Self::Output {
        let mut polynomial = Polynomial::new();

        // Combine storage and validity flag
        polynomial.storage.store(
            self.storage.load(Ordering::Acquire) + rhs.storage.load(Ordering::Acquire),
            Ordering::Release,
        );
        polynomial.valid.store(
            self.valid.load(Ordering::Acquire) && rhs.valid.load(Ordering::Acquire),
            Ordering::Release,
        );

        // Combine products of both
        for (factors, sub_polynomial) in &self.products {
            polynomial
                .products
                .push((factors.clone(), sub_polynomial.clone()));
        }
        for (factors, sub_polynomial) in &rhs.products {
            polynomial
                .products
                .push((factors.clone(), sub_polynomial.clone()));
        }

        polynomial
    }
}

impl Mul<u16> for Polynomial {
    type Output = Polynomial;

    fn mul(self, rhs: u16) -> Self::Output {
        let mut polynomial = Polynomial::new();

        // Invalidate stored value
        polynomial.valid.store(false, Ordering::Release);

        // Combine own products with new leaf index
        for (factors, sub_polynomial) in &self.products {
            let mut extended = factors.clone();
            extended.insert(rhs);

            polynomial.products.push((extended, sub_polynomial.clone()));
        }

        polynomial
    }
}
