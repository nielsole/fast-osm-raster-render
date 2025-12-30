# Python Test Suite for Rust OSM Renderer

Comprehensive regression and functional test suite for the Rust OSM renderer.

## Quick Start

```bash
# Install dependencies
cd python_tests
pip install -r requirements.txt

# Build the renderer first
cd ..
cargo build --example render_tile

# Run all tests
cd python_tests
pytest
```

## Test Categories

### Regression Tests (`test_regression.py`)
- **Purpose**: Pixel-perfect comparison against golden images
- **Detects**: Any visual changes in rendering output
- **Run with**: `pytest -m regression`

### Unit Tests (`test_unit.py`)
- **Purpose**: Threshold-based functional validation
- **Detects**: Major rendering failures, edge case handling
- **Run with**: `pytest -m unit`

## Test Data

### Automatic Generation
Test fixtures are generated automatically on first run. They include:
- `cross_pattern.osm.pbf` - Horizontal + vertical line
- `horizontal_line.osm.pbf` - Single horizontal line
- `vertical_line.osm.pbf` - Single vertical line
- `diagonal_line.osm.pbf` - Diagonal line
- `grid_pattern.osm.pbf` - 5x5 grid
- `empty_tile.osm.pbf` - No visible content
- `single_point.osm.pbf` - Degenerate line
- `boundary_cross.osm.pbf` - Ways crossing tile edges

### Manual Generation
```bash
cd python_tests
python test_data/generator.py
```

## Golden Images

On first run, regression tests will skip with a message about missing golden images.

### Generate Initial Golden Images

```bash
# Run tests once to generate outputs
pytest test_regression.py -v

# Review the outputs in /tmp/pytest-*
ls /tmp/pytest-*/

# If satisfied, save as golden images
python scripts/update_golden_images.py
```

### Update Golden Images After Changes

When you intentionally change rendering (shader improvements, bug fixes):

```bash
# 1. Run regression tests (they will fail)
pytest test_regression.py -v

# 2. Review diff images in test output directories
# 3. If changes are correct, update goldens
python scripts/update_golden_images.py

# 4. Commit the updated golden images
git add golden_images/
git commit -m "Update golden images for shader improvements"
```

## Running Tests

```bash
# All tests
pytest

# Only regression tests
pytest -m regression

# Only unit tests
pytest -m unit

# Verbose output
pytest -v

# Show print statements
pytest -s

# Specific test
pytest test_unit.py::TestGeometricPatterns::test_cross_pattern_coverage

# Parallel execution (faster)
pytest -n auto
```

## Test Output

### Successful Test
```
test_regression.py::TestRegressionSuite::test_render_matches_golden[cross_pattern-11-1024-1024-Cross pattern-mercator] PASSED
```

### Failed Regression Test
```
FAILED test_regression.py::TestRegressionSuite::test_render_matches_golden
AssertionError: Rendering differs from golden image:
  Fixture: cross_pattern (Cross pattern)
  Shader: mercator
  Different pixels: 1,523 (2.32%)
  Diff image: /tmp/pytest-xyz/cross_pattern_mercator_diff.png

If this change is intentional, update golden images with:
  python scripts/update_golden_images.py
```

## Requirements

- Python 3.8+
- osmium-tool (for PBF generation)
- Vulkan-capable GPU (or SwiftShader for CI)
- render_tile binary (cargo build --example render_tile)

### Install Dependencies

**Ubuntu/Debian:**
```bash
sudo apt-get install osmium-tool python3-pip
pip install -r requirements.txt
```

**macOS:**
```bash
brew install osmium-tool
pip install -r requirements.txt
```

## Troubleshooting

### "render_tile binary not found"
```bash
cd ..
cargo build --example render_tile
# or for optimized build:
cargo build --release --example render_tile
```

### "osmium command not found"
```bash
sudo apt-get install osmium-tool
```

### "Vulkan not available"
For CI environments without GPU:
```bash
# Install SwiftShader (software Vulkan)
# Set environment variable
export VK_ICD_FILENAMES=/path/to/swiftshader_icd.json
```

Or skip GPU tests:
```bash
pytest -m "not regression and not unit"
```

## CI/CD Integration

```yaml
# .github/workflows/test.yml
- name: Install dependencies
  run: |
    sudo apt-get install osmium-tool
    pip install -r python_tests/requirements.txt

- name: Build renderer
  run: cargo build --example render_tile

- name: Run tests
  run: |
    cd python_tests
    pytest -v
```

## Directory Structure

```
python_tests/
├── README.md                   # This file
├── requirements.txt            # Python dependencies
├── conftest.py                 # Pytest fixtures
├── test_regression.py          # Regression tests
├── test_unit.py                # Unit tests
├── utils/                      # Utility modules
│   ├── renderer.py             # Render tile wrapper
│   ├── image_comparison.py     # Image analysis
│   └── osm_builder.py          # OSM data builder
├── test_data/                  # Test data
│   ├── generator.py            # Generate test PBF files
│   └── fixtures/               # Generated PBF files (gitignored)
├── golden_images/              # Reference images for regression
└── scripts/                    # Maintenance scripts
    ├── generate_test_data.py   # Regenerate test data
    └── update_golden_images.py # Update golden images
```

## Writing New Tests

### Add a New Test Fixture

1. Edit `test_data/generator.py`:
```python
def generate_my_pattern(output_dir: Path) -> None:
    builder = OSMBuilder()
    # Add your geometry
    builder.add_line(...)
    builder.build_to_pbf(output_dir / "my_pattern.osm.pbf")
```

2. Add to `generate_all_test_data()` function

3. Add test case to `test_regression.py`:
```python
TEST_CASES = [
    # existing cases...
    ("my_pattern", 10, 512, 512, "My pattern description"),
]
```

### Add a New Unit Test

```python
def test_my_validation(render_tile_binary, test_data_dir, temp_output_dir):
    pbf_file = test_data_dir / "my_pattern.osm.pbf"
    output = temp_output_dir / "my_test.png"

    runner = RenderTileRunner(render_tile_binary)
    result = runner.render(pbf_file, 10, 512, 512, output)

    assert result.success
    analyzer = ImageAnalyzer(output)
    # Your assertions...
```

## Performance Considerations

- First run is slow (generates test data, builds index)
- Subsequent runs reuse generated fixtures
- Parallel execution: `pytest -n auto` (requires pytest-xdist)
- Skip slow tests: `pytest -m "not slow"`

## License

Same as parent project.
