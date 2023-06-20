use ndarray::{Array1, Array2};

pub struct LinearModel {
    forward_model: Array2<f32>,
    input_model: Array2<f32>,
    output_model: Array2<f32>,
}

impl LinearModel {
    pub fn new(
        forward_model: Array2<f32>,
        input_model: Array2<f32>,
        output_model: Array2<f32>,
    ) -> Self {
        Self {
            forward_model,
            input_model,
            output_model,
        }
    }

    pub fn forward(&self, state: &Array1<f32>, input: &Array1<f32>) -> Array1<f32> {
        self.forward_model.dot(state) + self.input_model.dot(input)
    }

    pub fn measure(&self, state: &Array1<f32>) -> Array1<f32> {
        self.output_model.dot(state)
    }
}

struct Kalman {
    // Gaussian estimation of state
    prediction: Array1<f32>,
    estimate: Array1<f32>,

    // The model of the tracked process
    model: LinearModel,

    // Noise as covariance matrices
    process_noise: Array2<f32>,
    sensor_noise: Array2<f32>,

    // Kalman values
    residual: Array1<f32>,
    residual_covariance: Array2<f32>,
    kalman_gain: Array2<f32>,
}

impl Kalman {}

// class Kalman:

//     """The Kalman filter for linear state estimation.

//     The Kalman filter is a single target tracker for linear state space models, i.e. models that
//     describe the transition of a state variable and its relationship to sensor readings
//     as matrix-vector-multiplications.
//     Additionally, the Kalman filter is based on the assumption that the state process and
//     measurements are sampled from a Gaussian distribution.

//     Examples:
//         First, import some helper functions from numpy.

//         >>> from numpy import array
//         >>> from numpy import eye
//         >>> from numpy import vstack

//         Then, setup the system's model.
//         In this case, we track a 1D position that we assume to have a constant velocity.
//         Thereby we choose the transition model and measurement function like so.

//         >>> F = array([[1.0, 1.0], [0.0, 0.0]])
//         >>> H = array([[1.0, 0.0]])

//         Furthermore, we assume the following covariance matrices to model
//         the noise in our model and measurements.

//         >>> Q = eye(2)
//         >>> R = eye(1)

//         Our initial belief is a position and velocity of 0.

//         >>> mean = vstack([0.0, 0.0])
//         >>> covariance = array([[1.0, 0.0], [0.0, 1.0]])
//         >>> estimate = Gaussian(mean, covariance)

//         Then, we initialize the filter.
//         Since, this model has not input we can ignore the control function B.

//         >>> kalman = Kalman(F, estimate, H, Q, R)

//         Now, we can predict based on the provided model and correct predictions with
//         measurements of the true position.

//         >>> kalman.predict()
//         >>> kalman.correct(array([5.]))

//         Predictions and corrections do not need to alternate every time.
//         As an example, you can predict the state multiple times should your measurements be
//         unavailable for an extended period of time.

//     Args:
//         F: State transition model, i.e. the change of x in a single timestep (n, n)
//         estimate: Initial belief, i.e. the gaussian distribution that describes your initial guess
//             on the target's state
//         H: Measurement model, i.e. a mapping from a state to measurement space (m, n)
//         Q: Process noise matrix, i.e. the covariance of the state transition (n, n)
//         R: Measurement noise matrix, i.e. the covariance of the sensor readings (m, m)
//         B: Input dynamics model, i.e. the influence of a set system input on the state transition (1, k)
//         keep_trace: Flag for tracking filter process

//     References:
//         - https://en.wikipedia.org/wiki/Kalman_filter
//     """

//     # In this context, we reproduce a common filter notation
//     # pylint: disable=invalid-name
//     # pylint: disable=too-many-instance-attributes, too-many-arguments

//     def __init__(
//         self,
//         F: Union[ndarray, Callable[..., ndarray]],
//         estimate: Gaussian,
//         H: Union[ndarray, Callable[..., ndarray]],
//         Q: ndarray,
//         R: ndarray,
//         B: Optional[ndarray] = None,
//         keep_trace: bool = False,
//     ):
//         # Initial belief
//         self.estimate = deepcopy(estimate)
//         self.prediction = deepcopy(estimate)

//         # Model specification
//         self.F = F
//         self.B = B
//         self.H = H
//         self.Q = Q
//         self.R = R

//         # Residual and its covariance matrix
//         self.y: ndarray
//         self.S: ndarray

//         # Kalman gain
//         self.K: ndarray

//         # Objects for process tracing
//         self.keep_trace = keep_trace
//         self.predictions = DataFrame(columns=["x", "P", "F"])
//         self.estimates = DataFrame(columns=["x", "P", "z"])

//     def predict(self, **kwargs) -> None:
//         """Predict a future state based on a linear forward model with optional system input."""

//         # Compute F if additional parameters are needed
//         F = self.F(**kwargs) if callable(self.F) else self.F

//         # Predict next state
//         self.prediction.x = F @ self.estimate.x
//         self.prediction.P = F @ self.estimate.P @ F.T + self.Q

//         # Consider system input
//         u = kwargs.pop("u", None)
//         if u is not None:
//             self.prediction.x += self.B @ u

//         # Append prediction data to trace
//         if self.keep_trace:
//             new = DataFrame(
//                 {"x": (self.prediction.x.copy(),), "P": (self.prediction.P.copy(),), "F": (F.copy(),)}
//             )
//             self.predictions = concat([self.predictions, new], ignore_index=True)

//     def correct(self, z: ndarray, **kwargs) -> None:
//         """Correct a state prediction based on a measurement.

//         Args:
//             z: The measurement taken at this timestep
//         """

//         # Check for differing measurement model
//         H = kwargs.pop("H", self.H)

//         # Compute H if additional parameters are needed
//         if callable(H):
//             H = H(**kwargs)

//         # Compute the residual and its covariance
//         self.y = z - H @ self.prediction.x
//         self.S = H @ self.prediction.P @ H.T + self.R

//         # Compute the new Kalman gain
//         self.K = self.prediction.P @ H.T @ inv(self.S)

//         # Estimate new state
//         self.estimate.x = self.prediction.x + self.K @ self.y
//         self.estimate.P = self.prediction.P - self.K @ self.S @ self.K.T

//         # Append estimation data to trace
//         if self.keep_trace:
//             new = DataFrame(
//                 {"x": (self.estimate.x.copy(),), "P": (self.estimate.P.copy(),), "z": (z.copy(),)}
//             )
//             self.estimates = concat([self.estimates, new], ignore_index=True)
