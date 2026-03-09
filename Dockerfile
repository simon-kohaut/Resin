# Base image from Rust project
FROM ubuntu:22.04

# Setup dependencies
RUN apt-get update && apt-get install -y software-properties-common
RUN add-apt-repository ppa:potassco/stable
RUN apt-get update && apt-get install -y \
    git clingo graphviz curl build-essential \
    && rm -rf /var/lib/apt/lists/*

# Setup Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/.cargo/bin:${PATH}"

# Obtain repository
WORKDIR /Resin
RUN git clone https://github.com/simon-kohaut/Resin.git .

# Ensure environment is setup on entry
# Using single quotes prevents shell expansion during build
RUN echo '#!/bin/bash' > /environment_setup.sh && \
    echo 'export CLINGO_LIBRARY_PATH=/usr/lib' >> /environment_setup.sh && \
    echo 'export LD_LIBRARY_PATH=/usr/lib:$LD_LIBRARY_PATH' >> /environment_setup.sh && \
    echo 'exec "$@"' >> /environment_setup.sh

RUN chmod +x /environment_setup.sh
ENTRYPOINT ["/environment_setup.sh"]
CMD ["bash"]