FROM debian:12-slim

# Update package manager and install required tools
RUN apt-get update && apt-get install -y \
    ca-certificates wget && \
    rm -rf /var/lib/apt/lists/*

# Install glibc 2.39
RUN wget https://ftp.gnu.org/gnu/libc/glibc-2.39.tar.gz && \
    tar -xvzf glibc-2.39.tar.gz && \
    cd glibc-2.39 && \
    mkdir build && cd build && \
    ../configure --prefix=/opt/glibc-2.39 && \
    make -j$(nproc) && \
    make install && \
    rm -rf /glibc-2.39 /glibc-2.39.tar.gz

# Set the environment to use the new glibc
ENV LD_LIBRARY_PATH=/opt/glibc-2.39/lib:$LD_LIBRARY_PATH

# Copy your files
COPY genesis /etc/genesis

ARG TARGETARCH
COPY bin/dolos-Linux-${TARGETARCH} /bin/dolos
RUN chmod +x /bin/dolos
RUN ln -s /bin/dolos /dolos

ENTRYPOINT [ "dolos" ]
