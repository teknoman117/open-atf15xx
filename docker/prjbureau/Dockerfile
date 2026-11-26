# Image for running whitequark's prjbureau flow end to end:
#   Verilog --(Yosys)--> EDIF/BLIF --(vendor fit150x.exe under wine)--> JED --(util.fuseconv)--> SVF
# plus the Python utilities (util.fusecli, util.fuseconv, database) and our own analysis scripts.
#
# The vendor fitters are NOT part of the image (they are proprietary): mount a directory that
# contains fit1502.exe / fit1504.exe / fit1508.exe (e.g. WinCUPL's Fitters directory) at /vendor.
#
# Build:  docker build -t open-atf150x-prjbureau docker/prjbureau/
# Run:    docker/prjbureau/run.sh <command>
FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
RUN dpkg --add-architecture i386 \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
      yosys python3 python3-bitarray python3-pytest python3-venv git ca-certificates make \
      wine wine32:i386 \
 && rm -rf /var/lib/apt/lists/*

ENV WINEDEBUG=-all WINEARCH=win32 PYTHONDONTWRITEBYTECODE=1
WORKDIR /work
