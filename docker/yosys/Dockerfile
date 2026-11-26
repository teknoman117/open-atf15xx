# Open Verilog → ATF15xx flow image: Yosys built from a pinned upstream release, Icarus Verilog
# for co-simulation, and a Rust toolchain for the workspace crates (atf15xx-synth).
#
# Build:  docker build -t open-atf150x-yosys docker/yosys/
# Run:    docker/yosys/run.sh <command>     (workspace mounted at /work, runs as the host user)
#
# Yosys: v0.71, tag commit b4c586fd37d0ec61bb38fc1a85fd6a2fde48fcd1 (ISC licence).

FROM ubuntu:24.04 AS yosys-build
ARG DEBIAN_FRONTEND=noninteractive
ARG YOSYS_TAG=v0.71
ARG YOSYS_COMMIT=b4c586fd37d0ec61bb38fc1a85fd6a2fde48fcd1
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential clang lld cmake bison flex gawk git make ca-certificates pkg-config python3 \
      libreadline-dev tcl-dev libffi-dev libfl-dev zlib1g-dev \
 && rm -rf /var/lib/apt/lists/*
# Yosys >= 0.6x builds with CMake (presets in CMakePresets.json)
RUN git clone --branch ${YOSYS_TAG} --depth 1 --recurse-submodules --shallow-submodules \
      https://github.com/YosysHQ/yosys /src/yosys \
 && cd /src/yosys && test "$(git rev-parse HEAD)" = "${YOSYS_COMMIT}" \
 && cmake --preset release -DCMAKE_INSTALL_PREFIX=/opt/yosys -DYOSYS_WITH_PYTHON=OFF \
 && cmake --build build-rel --parallel "$(nproc)" \
 && cmake --install build-rel \
 && /opt/yosys/bin/yosys -V

FROM ubuntu:24.04
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libreadline8t64 tcl libffi8 zlib1g libfl2 python3 iverilog ca-certificates curl build-essential git \
 && rm -rf /var/lib/apt/lists/*
COPY --from=yosys-build /opt/yosys /opt/yosys
RUN curl -sSf https://sh.rustup.rs | RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo sh -s -- -y --profile minimal \
 && chmod -R a+rX /opt/rustup /opt/cargo
ENV RUSTUP_HOME=/opt/rustup PATH="/opt/yosys/bin:/opt/cargo/bin:${PATH}"
WORKDIR /work
