FROM ubuntu:24.04 AS build
ARG VERSION=1.7.0
ENV DEBIAN_FRONTEND=noninteractive VCPKG_FORCE_SYSTEM_BINARIES=1 PATH=/root/.cargo/bin:$PATH
RUN apt-get update && apt-get install -y sudo curl ca-certificates git build-essential just
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
RUN git clone --depth 1 --branch v${VERSION} https://github.com/gyroflow/gyroflow.git /gyroflow
WORKDIR /gyroflow
# vcpkg's OpenCV no longer builds tegra_hal on arm64, which the build script still links
RUN sed -i 's/,tegra_hal,png/,png/' _scripts/common.just
RUN just install-deps
# LTO needs more memory than Docker Desktop's default VM has, and the prebuilt ffmpeg leaves text shaping symbols undefined
ENV CARGO_PROFILE_DEPLOY_LTO=false CARGO_PROFILE_DEPLOY_CODEGEN_UNITS=16 RUSTFLAGS="-C link-arg=-Wl,--allow-shlib-undefined"
# compiles, then fails while packaging - the two steps below fix the packaging and keep this layer cached
RUN just deploy || true
# Qt for arm64 ships another ICU version, and the BRAW and R3D plugins do not exist for arm64
RUN sed -i -e 's|"\$QT_DIR/lib/libicu\([a-z0-9]*\)\.so\.56"|"$QT_DIR"/lib/libicu\1.so.*|' -e '/libmdk-braw.so\|libmdk-r3d.so/ s/$/ || true/' _scripts/linux.just
# the archive is written before the AppImage step, which is not needed and fails
RUN just deploy || test -f _deployment/_binaries/Gyroflow-linux-arm64.tar.gz
# the prebuilt ffmpeg links libc++, which the computer image does not have
RUN cd _deployment/_binaries && tar -xzf Gyroflow-linux-arm64.tar.gz && cp -L /usr/lib/aarch64-linux-gnu/libc++.so.1 /usr/lib/aarch64-linux-gnu/libc++abi.so.1 /usr/lib/aarch64-linux-gnu/libunwind.so.1 Gyroflow/lib/ && tar -czf Gyroflow-linux-arm64.tar.gz Gyroflow

FROM scratch
COPY --from=build /gyroflow/_deployment/_binaries/Gyroflow-linux-arm64.tar.gz /
