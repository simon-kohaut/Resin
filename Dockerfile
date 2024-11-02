# Base image sets up ROS2 on Ubuntu
FROM ros:iron-ros-base-jammy

# Resolve APT dependencies
RUN apt-get update -qq && apt-get upgrade -y
RUN apt install liblapack-dev liblapack3 libopenblas-base libopenblas-dev -y
RUN apt-get install curl git libclang-dev python3-pip python3-vcstool software-properties-common -y
RUN add-apt-repository ppa:potassco/stable
RUN apt-get update -qq
RUN apt-get install gringo -y

# Create and use new user with fixed ID and enable the installation of dependencies
RUN useradd --create-home --shell /bin/bash --uid 1000 developer
RUN usermod -aG sudo developer
RUN echo "developer ALL=(ALL) NOPASSWD: ALL" >> /etc/sudoers && cat /etc/sudoers
USER developer

# Environment settings
ENV HOME=/home/developer
ENV ROS_DISTRO=iron
ENV TERM=xterm-256color
ENV PATH=$HOME/.local/bin:$PATH
ENV CLINGO_LIBRARY_PATH=/lib

# Setup .bahrc to have ROS2 and Rust available
RUN echo '# Environment setup' >> $HOME/.bashrc
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
RUN echo 'source /opt/ros/$ROS_DISTRO/setup.bash' >> $HOME/.bashrc

# Setup Rust with ROS2 bindings
# Reference: https://github.com/ros2-rust/ros2_rust
WORKDIR $HOME
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
RUN . .cargo/env && cargo install cargo-ament-build 
RUN pip install git+https://github.com/colcon/colcon-cargo.git
RUN pip install git+https://github.com/colcon/colcon-ros-cargo.git

WORKDIR $HOME/resin
RUN mkdir -p external/src
RUN git clone https://github.com/ros2-rust/ros2_rust.git external/src/ros2_rust
RUN vcs import external/src < src/ros2_rust/ros2_rust_iron.repos
RUN colcon build
