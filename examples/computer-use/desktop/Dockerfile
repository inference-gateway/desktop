FROM ubuntu:24.04

ARG INFER_VERSION=v0.245.0
ARG TARGETARCH
ARG INFER_RELEASE=https://github.com/inference-gateway/cli/releases/download/${INFER_VERSION}
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl git xvfb openbox xterm fonts-dejavu && \
    install -d -m 0755 /etc/apt/keyrings && \
    curl -fsSL https://packages.mozilla.org/apt/repo-signing-key.gpg -o /etc/apt/keyrings/packages.mozilla.org.asc && \
    echo "deb [signed-by=/etc/apt/keyrings/packages.mozilla.org.asc] https://packages.mozilla.org/apt mozilla main" > /etc/apt/sources.list.d/mozilla.list && \
    printf 'Package: *\nPin: origin packages.mozilla.org\nPin-Priority: 1000\n' > /etc/apt/preferences.d/mozilla && \
    apt-get update && \
    apt-get install -y --no-install-recommends firefox && \
    rm -rf /var/lib/apt/lists/*

RUN cd /tmp && \
    curl -fsSLO ${INFER_RELEASE}/infer-linux-${TARGETARCH}.tar.gz && \
    curl -fsSLO ${INFER_RELEASE}/checksums.txt && \
    grep " infer-linux-${TARGETARCH}.tar.gz$" checksums.txt | sha256sum -c - && \
    tar -xzf infer-linux-${TARGETARCH}.tar.gz -C /usr/local/bin && \
    rm infer-linux-${TARGETARCH}.tar.gz checksums.txt && \
    infer version

RUN userdel -r ubuntu && \
    useradd -m -u 1000 -s /bin/bash infer && \
    mkdir -p /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix
COPY entrypoint.sh /usr/local/bin/desktop-entrypoint
COPY policies.json /etc/firefox/policies/policies.json
COPY scenarios.yaml /opt/agent-computer/scenarios.yaml
RUN chmod +x /usr/local/bin/desktop-entrypoint

USER infer
WORKDIR /home/infer

ENV DISPLAY=:99
ENV INFER_IN_CONTAINER=true
ENV INFER_GATEWAY_RUN=false

RUN mkdir -p .infer && infer init

ENTRYPOINT ["desktop-entrypoint"]
