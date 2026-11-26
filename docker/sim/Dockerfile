# Simulation / equivalence-checking image for the ATF15xx JED simulator (tools/sim/).
#
# Extends open-atf150x-tools (Yosys 0.33 + yosys-abc + Rust, docker/Dockerfile) with Icarus Verilog,
# SMT solvers, and Verilator built from source at a pinned release (tools/sim/vcompare.py).
#
# Build:  docker build -t open-atf150x-tools docker/ && docker build -t open-atf150x-sim docker/sim/
# Run:    IMAGE=open-atf150x-sim docker/run.sh <command>   (tools/sim/run.sh does this)

# ---- Verilator from source (pinned) ------------------------------------------------------------
FROM open-atf150x-tools AS verilator-build
ARG DEBIAN_FRONTEND=noninteractive
# v5.054, tag object 76d93f46…, commit bbb60a403932a9f695cb6ff001ceb73de6ad9234
ARG VERILATOR_TAG=v5.054
ARG VERILATOR_COMMIT=bbb60a403932a9f695cb6ff001ceb73de6ad9234
RUN apt-get update \
 && apt-get install -y --no-install-recommends git ca-certificates autoconf flex bison help2man \
        perl python3 make g++ libfl2 libfl-dev zlib1g-dev \
 && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch "$VERILATOR_TAG" https://github.com/verilator/verilator /src/verilator \
 && cd /src/verilator \
 && test "$(git rev-parse HEAD)" = "$VERILATOR_COMMIT" \
 && autoconf && ./configure --prefix=/opt/verilator \
 && make -j"$(nproc)" && make install

# ---- final image -------------------------------------------------------------------------------
FROM open-atf150x-tools
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends iverilog z3 boolector \
        perl python3 make g++ libfl2 zlib1g \
 && rm -rf /var/lib/apt/lists/*
COPY --from=verilator-build /opt/verilator /opt/verilator
ENV PATH=/opt/verilator/bin:$PATH
RUN verilator --version
