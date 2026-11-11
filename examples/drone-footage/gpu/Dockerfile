FROM ubuntu:26.04 AS build
ENV DEBIAN_FRONTEND=noninteractive
RUN sed -i 's/^Types: deb$/Types: deb deb-src/' /etc/apt/sources.list.d/ubuntu.sources && apt-get update && apt-get build-dep -y mesa && apt-get install -y dpkg-dev
WORKDIR /src
RUN apt-get source mesa && mv mesa-*/ mesa
WORKDIR /src/mesa
# Mesa's Venus driver fails on a Mac's 16 KiB memory pages without this downstream change
ADD --checksum=sha256:a460ec3420aea7d35e0e96cecdeed2dc67194c027f3d9013ad97c7e896c65a65 https://gitlab.freedesktop.org/slp/mesa/-/commit/761ef1ec5ff2aae1cc3dc8bbc22b3d06ef04b549.diff /src/venus-16k.diff
RUN patch -p1 < /src/venus-16k.diff
RUN meson setup build -Dbuildtype=release -Dvulkan-drivers=virtio -Dgallium-drivers= -Dplatforms= -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dopengl=false -Dgles1=disabled -Dgles2=disabled -Dllvm=disabled -Dvideo-codecs= -Dtools= && ninja -C build src/virtio/vulkan/libvulkan_virtio.so

FROM scratch
COPY --from=build /src/mesa/build/src/virtio/vulkan/libvulkan_virtio.so /
