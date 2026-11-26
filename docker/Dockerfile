# Toolchain image for the open-atf150x Verilog → ATF15xx experiments.
#
# Contains Yosys (from Ubuntu 24.04) and the Python dependencies of whitequark's prjbureau
# (bitarray) so that its utilities (util.fuseconv, util.fusecli, the database) can be run.
# The vendor fitters are NOT included: they run on the host under wine (see tools/fuzzers/).
#
# Build:  docker build -t open-atf150x-tools docker/
# Run:    docker/run.sh <command>      (e.g. docker/run.sh cargo test --workspace)
FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      yosys python3 python3-pip python3-venv python3-bitarray git ca-certificates make \
 && rm -rf /var/lib/apt/lists/*

# Rust toolchain for building/testing the workspace inside the container as well; libclang for
# bindgen (espresso-logic, the Espresso oracle in atf15xx-fit/tests/oracles.rs).
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl build-essential libclang-dev clang cmake \
 && rm -rf /var/lib/apt/lists/* \
 && curl -sSf https://sh.rustup.rs | RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo sh -s -- -y --profile minimal \
 && RUSTUP_HOME=/opt/rustup /opt/cargo/bin/rustup component add clippy rustfmt \
 && chmod -R a+rX /opt/rustup /opt/cargo
# Toolchain is world-readable so the container can run as the host user (docker/run.sh);
# the registry cache and target dir live in the mounted .scratch/.
ENV RUSTUP_HOME=/opt/rustup PATH="/opt/cargo/bin:${PATH}"

WORKDIR /work
