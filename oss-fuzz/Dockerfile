# OSS-Fuzz Dockerfile for cyrs.
#
# This file is the artifact submitted upstream to google/oss-fuzz at
# projects/cyrs/Dockerfile. It is NOT built by cyrs's own CI — see
# oss-fuzz/README.md for the onboarding flow.
#
# Docs: https://google.github.io/oss-fuzz/getting-started/new-project-guide/rust-lang/

FROM gcr.io/oss-fuzz-base/base-builder-rust

# Clone cyrs at the revision ClusterFuzz asks for. OSS-Fuzz invokes this
# Dockerfile with the repo URL / ref wired in as build args on its side;
# we hard-code the HTTPS clone here to mirror what upstream projects do.
RUN git clone --depth 1 https://github.com/phall/cyrs.git $SRC/cyrs

WORKDIR $SRC/cyrs

# `zip` is used by build.sh to bundle the seed corpora; `build-essential`
# is already installed by base-builder-rust.
RUN apt-get update && \
    apt-get install -y --no-install-recommends zip && \
    rm -rf /var/lib/apt/lists/*

# build.sh assumes it's run from $SRC/cyrs/fuzz; copying it to the image
# root (which is the OSS-Fuzz convention) lets ClusterFuzz execute it
# directly.
COPY build.sh $SRC/
