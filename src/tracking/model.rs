use super::{Matrix, Vector};

#[derive(Clone)]
pub struct LinearModel {
    pub forward_model: Matrix,
    pub input_model: Matrix,
    pub output_model: Matrix,
}

impl LinearModel {
    pub fn new(forward_model: &Matrix, input_model: &Matrix, output_model: &Matrix) -> Self {
        Self {
            forward_model: forward_model.clone(),
            input_model: input_model.clone(),
            output_model: output_model.clone(),
        }
    }

    pub fn get_state_dimension(&self) -> usize {
        self.forward_model.shape()[0]
    }

    pub fn get_measurement_dimension(&self) -> usize {
        self.output_model.shape()[1]
    }

    pub fn forward(&self, state: &Vector, input: Option<&Vector>) -> Vector {
        match input {
            Some(vector) => self.forward_model.dot(state) + self.input_model.dot(vector),
            None => self.forward_model.dot(state),
        }
    }

    pub fn measure(&self, state: &Vector) -> Vector {
        self.output_model.dot(state)
    }
}
