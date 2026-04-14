# Upstream PyPI package + version: CI passes --build-arg from tools/mcp-tools.json (`upstream_mcp_pypi`).
FROM python:3.12-slim-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends git git-lfs \
    && rm -rf /var/lib/apt/lists/* \
    && git lfs install --system
ARG UPSTREAM_MCP_PYPI_PACKAGE=mcp-server-git
ARG UPSTREAM_MCP_PYPI_VERSION=2026.1.14
ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PYTHONUNBUFFERED=1
RUN pip install --no-cache-dir "${UPSTREAM_MCP_PYPI_PACKAGE}==${UPSTREAM_MCP_PYPI_VERSION}"
RUN useradd -r -u 10001 -m -d /home/mcp mcp
USER mcp
ENTRYPOINT ["mcp-server-git"]
