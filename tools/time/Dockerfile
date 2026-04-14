# Upstream PyPI package + version: CI passes --build-arg from tools/mcp-tools.json (`upstream_mcp_pypi`).
FROM python:3.12-slim-bookworm
ARG UPSTREAM_MCP_PYPI_PACKAGE=mcp-server-time
ARG UPSTREAM_MCP_PYPI_VERSION=2026.1.26
ENV PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PYTHONUNBUFFERED=1
RUN pip install --no-cache-dir "${UPSTREAM_MCP_PYPI_PACKAGE}==${UPSTREAM_MCP_PYPI_VERSION}"
RUN useradd -r -u 10001 -m -d /home/mcp mcp
USER mcp
ENTRYPOINT ["mcp-server-time"]
