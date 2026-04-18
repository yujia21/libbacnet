# Contributing

## Build

### Prerequisites

- Rust 1.75+ (`rustup toolchain install stable`)
- Python 3.12+
- [`maturin`](https://maturin.rs) (`uv tool install maturin`)

### Development build

```bash
# clone and enter the repo
git clone https://github.com/example/libbacnet
cd libbacnet

# create a virtual environment and install dev dependencies
uv sync

# compile the Rust extension and install it into the venv (editable)
maturin develop
```

### Release build (wheel)

```bash
maturin build --release
uv pip install target/wheels/libbacnet-*.whl
```

---

## Running tests

```bash
# Unit and integration tests (no real device required)
uv run pytest -v

# Rust unit tests (activate the venv first so PyO3 links against the correct Python)
source .venv/bin/activate
cargo test

# Linting
cargo clippy
cargo fmt --check
uv run ruff format python/ tests/
uv run ruff check python/ tests/
```
