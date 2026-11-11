ARG COMPUTER=ghcr.io/inference-gateway/computer:latest

FROM ubuntu:26.04 AS mesa
ENV DEBIAN_FRONTEND=noninteractive
RUN sed -i 's/^Types: deb$/Types: deb deb-src/' /etc/apt/sources.list.d/ubuntu.sources && apt-get update && apt-get build-dep -y mesa && apt-get install -y dpkg-dev
WORKDIR /src
RUN apt-get source mesa && mv mesa-*/ mesa
WORKDIR /src/mesa
COPY venus-16k.patch /src/
RUN patch -p1 < /src/venus-16k.patch
RUN meson setup build -Dbuildtype=release -Dvulkan-drivers=virtio -Dgallium-drivers= -Dplatforms= -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dopengl=false -Dgles1=disabled -Dgles2=disabled -Dllvm=disabled -Dvideo-codecs= -Dtools= && ninja -C build src/virtio/vulkan/libvulkan_virtio.so

FROM ${COMPUTER}
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg libvulkan1 mesa-vulkan-drivers vulkan-tools && rm -rf /var/lib/apt/lists/*
COPY --from=mesa /src/mesa/build/src/virtio/vulkan/libvulkan_virtio.so /usr/lib/aarch64-linux-gnu/libvulkan_virtio.so
