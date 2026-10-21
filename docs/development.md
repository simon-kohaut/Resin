# Development

## Building from source

Requirements: Rust toolchain, Clingo, Python ≥ 3.9, [Maturin](https://github.com/PyO3/maturin).

**macOS**
```bash
brew install clingo
export CLINGO_LIBRARY_PATH=$(brew --prefix clingo)/lib
maturin develop --release  # Optional for building the Python package
```

**Linux**
```bash
pip install clingo
CLINGO_DIR=$(python3 -c "import clingo, os; print(os.path.dirname(clingo.__file__))")
export CLINGO_LIBRARY_PATH="$CLINGO_DIR"
maturin develop --release  # Optional for building the Python package
```

## Running checks

```bash
cargo test
bash scripts/check.sh  # formatting, clippy and tests, as in CI
```

## Updating the Python type stub

After changing the Python API, rebuild the module and regenerate `resin.pyi` (this fails if the stub table in the script and the module disagree):

```bash
maturin develop --release
python scripts/generate_stub.py
```

## Pre-commit hook

Enable the hook once per clone; it formats staged Rust files and regenerates `resin.pyi` whenever `src/python_api.rs` changes (using the active virtualenv or `./venv`):

```bash
git config core.hooksPath scripts/hooks
```

## README animation

The animation at the top of the README is rendered with [manim](https://www.manim.community) (which needs Cairo and Pango, but no LaTeX) and ffmpeg:

```bash
pip install manim
python scripts/readme_animation.py  # writes docs/reactive_circuits_{dark,light}.gif
```
