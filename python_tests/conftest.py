"""Pytest configuration and fixtures for OSM renderer tests."""

import pytest
from pathlib import Path
import shutil

from utils.renderer import find_render_tile_binary
from test_data.generator import generate_all_test_data


@pytest.fixture(scope="session")
def project_root():
    """Get the project root directory."""
    return Path(__file__).parent.parent


@pytest.fixture(scope="session")
def render_tile_binary(project_root):
    """
    Locate and return path to render_tile binary.

    Searches for:
    1. target/release/examples/render_tile
    2. target/debug/examples/render_tile

    Returns:
        Path to the render_tile binary

    Raises:
        pytest.skip: If binary is not found
    """
    try:
        binary = find_render_tile_binary()
        return binary
    except FileNotFoundError as e:
        pytest.skip(str(e))


@pytest.fixture(scope="session")
def test_data_dir():
    """Return path to test_data/fixtures/ directory."""
    return Path(__file__).parent / "test_data" / "fixtures"


@pytest.fixture(scope="session")
def golden_images_dir():
    """Return path to golden_images/ directory."""
    return Path(__file__).parent / "golden_images"


@pytest.fixture(scope="session", autouse=True)
def generate_test_data(test_data_dir):
    """
    Auto-generate all test PBF files if missing.

    This runs once per test session before any tests.
    Skips generation if fixtures already exist.
    """
    test_data_dir.mkdir(parents=True, exist_ok=True)

    # Check if fixtures exist
    existing_fixtures = list(test_data_dir.glob("*.pbf"))

    if len(existing_fixtures) == 0:
        print("\nGenerating test data fixtures...")
        try:
            generate_all_test_data(test_data_dir)
        except Exception as e:
            pytest.fail(f"Failed to generate test data: {e}")
    else:
        print(f"\nFound {len(existing_fixtures)} existing test fixtures")


@pytest.fixture
def temp_output_dir(tmp_path):
    """
    Temporary directory for test outputs.

    Each test gets its own temporary directory that is cleaned up after.

    Returns:
        Path to temporary directory
    """
    return tmp_path


@pytest.fixture(scope="session")
def ensure_golden_images_dir(golden_images_dir):
    """Ensure golden images directory exists."""
    golden_images_dir.mkdir(parents=True, exist_ok=True)
    return golden_images_dir


def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line(
        "markers",
        "regression: Pixel-perfect regression tests against golden images"
    )
    config.addinivalue_line(
        "markers",
        "unit: Threshold-based functional tests"
    )
    config.addinivalue_line(
        "markers",
        "performance: Performance benchmark tests"
    )
    config.addinivalue_line(
        "markers",
        "slow: Tests that take more than 5 seconds"
    )


def pytest_collection_modifyitems(config, items):
    """Auto-mark slow tests based on timeout."""
    for item in items:
        # Mark tests with timeout > 5s as slow
        timeout_marker = item.get_closest_marker("timeout")
        if timeout_marker and timeout_marker.args[0] > 5:
            item.add_marker(pytest.mark.slow)
