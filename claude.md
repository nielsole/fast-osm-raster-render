# Claude Code Project Notes

## Python Dependency Management

**IMPORTANT**: This project uses `uv` for Python dependency management.

- ✅ **ALLOWED**: `uv sync`
- ❌ **NOT ALLOWED**: `pip install`, `uv pip install`

Always use `uv sync` to install Python dependencies from pyproject.toml or requirements.txt.
