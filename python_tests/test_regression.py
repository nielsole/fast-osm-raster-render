"""Pixel-perfect regression tests against golden images."""

import pytest
from pathlib import Path

from utils.renderer import RenderTileRunner
from utils.image_comparison import compare_images_exact


# Test cases: (fixture_name, z, x, y, description)
TEST_CASES = [
    ("cross_pattern", 11, 1024, 1024, "Cross pattern (horizontal + vertical)"),
    ("horizontal_line", 10, 512, 512, "Single horizontal line"),
    ("vertical_line", 10, 512, 512, "Single vertical line"),
    ("diagonal_line", 11, 1081, 660, "Diagonal line (bottom-left to top-right)"),
    ("grid_pattern", 12, 2048, 2048, "5x5 grid pattern"),
]


@pytest.mark.regression
@pytest.mark.parametrize("fixture_name,z,x,y,description", TEST_CASES)
@pytest.mark.parametrize("shader", ["mercator"])  # Start with mercator only
class TestRegressionSuite:
    """Pixel-perfect regression tests comparing against golden images."""

    def test_render_matches_golden(
        self,
        render_tile_binary,
        test_data_dir,
        golden_images_dir,
        temp_output_dir,
        fixture_name,
        z,
        x,
        y,
        description,
        shader,
    ):
        """
        Test that rendering produces pixel-perfect match with golden image.

        This is the main regression test - any pixel difference indicates
        a visual regression that needs investigation.
        """
        # Setup paths
        pbf_file = test_data_dir / f"{fixture_name}.osm.pbf"
        output = temp_output_dir / f"{fixture_name}_{shader}.png"
        golden = golden_images_dir / f"{fixture_name}_z{z}_x{x}_y{y}_{shader}.png"

        # Skip if PBF doesn't exist
        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        # Render
        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, z, x, y, output, shader)

        # Check rendering succeeded
        assert result.success, f"Rendering failed:\n{result.stderr}"
        assert output.exists(), f"Output file not created: {output}"

        # Compare against golden image
        if not golden.exists():
            # First run - save as golden
            pytest.skip(
                f"Golden image not found: {golden}\n"
                f"Run update_golden_images.py to generate it."
            )

        # Save diff to temp directory if comparison fails
        diff_path = temp_output_dir / f"{fixture_name}_{shader}_diff.png"

        comparison = compare_images_exact(output, golden, tolerance=0, save_diff=diff_path)

        assert comparison.matches, (
            f"Rendering differs from golden image:\n"
            f"  Fixture: {fixture_name} ({description})\n"
            f"  Shader: {shader}\n"
            f"  Different pixels: {comparison.diff_pixels:,} ({comparison.diff_percentage:.2f}%)\n"
            f"  Diff image: {diff_path if comparison.diff_image_path else 'N/A'}\n"
            f"  Output: {output}\n"
            f"  Golden: {golden}\n\n"
            f"If this change is intentional, update golden images with:\n"
            f"  python scripts/update_golden_images.py"
        )

    def test_render_succeeds(
        self,
        render_tile_binary,
        test_data_dir,
        temp_output_dir,
        fixture_name,
        z,
        x,
        y,
        description,
        shader,
    ):
        """
        Smoke test: verify rendering completes without errors.

        This is a fallback test for when golden images don't exist yet.
        """
        pbf_file = test_data_dir / f"{fixture_name}.osm.pbf"
        output = temp_output_dir / f"{fixture_name}_{shader}_smoke.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, z, x, y, output, shader)

        assert result.success, f"Rendering failed:\n{result.stderr}"
        assert output.exists(), f"Output file not created"
        assert output.stat().st_size > 0, "Output file is empty"

        # Verify it's a valid PNG
        from PIL import Image
        img = Image.open(output)
        assert img.size == (256, 256), f"Expected 256x256, got {img.size}"
        assert img.mode in ("RGB", "RGBA"), f"Unexpected image mode: {img.mode}"
