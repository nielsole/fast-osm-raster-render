"""Threshold-based functional tests."""

import pytest
from pathlib import Path

from utils.renderer import RenderTileRunner
from utils.image_comparison import ImageAnalyzer


@pytest.mark.unit
class TestGeometricPatterns:
    """Test rendering of geometric patterns with threshold validation."""

    def test_cross_pattern_coverage(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Cross pattern should have visible lines but not dominate the image."""
        pbf_file = test_data_dir / "cross_pattern.osm.pbf"
        output = temp_output_dir / "cross.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 11, 1024, 1024, output)

        assert result.success
        assert output.exists()

        analyzer = ImageAnalyzer(output)
        non_white_pct = analyzer.non_white_percentage()

        # Cross should be visible (>0.5%) but thin (<10%)
        assert 0.5 <= non_white_pct <= 10.0, (
            f"Expected 0.5-10% non-white pixels, got {non_white_pct:.2f}%"
        )

    def test_horizontal_line_coverage(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Horizontal line should render with expected coverage."""
        pbf_file = test_data_dir / "horizontal_line.osm.pbf"
        output = temp_output_dir / "horizontal.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 10, 512, 512, output)

        assert result.success

        analyzer = ImageAnalyzer(output)
        non_white_pct = analyzer.non_white_percentage()

        # Single horizontal line should be thin
        assert 0.3 <= non_white_pct <= 5.0, (
            f"Expected 0.3-5% non-white pixels for horizontal line, got {non_white_pct:.2f}%"
        )

    def test_vertical_line_coverage(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Vertical line should render with expected coverage."""
        pbf_file = test_data_dir / "vertical_line.osm.pbf"
        output = temp_output_dir / "vertical.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 10, 512, 512, output)

        assert result.success

        analyzer = ImageAnalyzer(output)
        non_white_pct = analyzer.non_white_percentage()

        # Single vertical line should be thin
        assert 0.3 <= non_white_pct <= 5.0, (
            f"Expected 0.3-5% non-white pixels for vertical line, got {non_white_pct:.2f}%"
        )

    def test_grid_pattern_coverage(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Grid pattern should have more coverage than single line."""
        pbf_file = test_data_dir / "grid_pattern.osm.pbf"
        output = temp_output_dir / "grid.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 12, 2048, 2048, output)

        assert result.success

        analyzer = ImageAnalyzer(output)
        non_white_pct = analyzer.non_white_percentage()

        # Grid has multiple lines, should be more visible
        assert 2.0 <= non_white_pct <= 20.0, (
            f"Expected 2-20% non-white pixels for grid, got {non_white_pct:.2f}%"
        )


@pytest.mark.unit
class TestEdgeCases:
    """Test edge cases and boundary conditions."""

    def test_empty_tile_is_white(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Empty tile (no ways in bbox) should render as all white."""
        pbf_file = test_data_dir / "empty_tile.osm.pbf"
        output = temp_output_dir / "empty.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 0, 0, 0, output)

        assert result.success

        analyzer = ImageAnalyzer(output)

        # Should be all white (or very close)
        assert analyzer.is_all_white() or analyzer.non_white_percentage() < 0.1, (
            f"Empty tile should be white, got {analyzer.non_white_percentage():.2f}% non-white"
        )

    def test_single_point_renders(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Degenerate line (single point) should render without crashing."""
        pbf_file = test_data_dir / "single_point.osm.pbf"
        output = temp_output_dir / "single_point.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 5, 16, 10, output)

        # Should succeed even if output is blank
        assert result.success
        assert output.exists()

    def test_boundary_cross_has_content(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Ways crossing tile boundaries should render visible content."""
        pbf_file = test_data_dir / "boundary_cross.osm.pbf"
        output = temp_output_dir / "boundary.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 10, 512, 512, output)

        assert result.success

        analyzer = ImageAnalyzer(output)
        non_white_count = analyzer.non_white_count()

        # Should have visible lines even if they extend beyond boundaries
        assert non_white_count > 100, (
            f"Expected >100 non-white pixels for boundary-crossing ways, got {non_white_count}"
        )


@pytest.mark.unit
class TestShaderVariants:
    """Test different shader types produce different outputs."""

    def test_different_shaders_produce_different_output(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Mercator and Simple shaders should produce different outputs."""
        pbf_file = test_data_dir / "cross_pattern.osm.pbf"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        mercator_output = temp_output_dir / "mercator.png"
        simple_output = temp_output_dir / "simple.png"

        runner = RenderTileRunner(render_tile_binary)

        # Render with both shaders
        result_mercator = runner.render(
            pbf_file, 11, 1024, 1024, mercator_output, shader_type="mercator"
        )
        result_simple = runner.render(
            pbf_file, 11, 1024, 1024, simple_output, shader_type="simple"
        )

        assert result_mercator.success
        assert result_simple.success

        # Compare images - they should be different
        from utils.image_comparison import compare_images_exact

        comparison = compare_images_exact(mercator_output, simple_output)

        # Different shaders should produce different results
        # (unless the projection happens to be identical for this tile)
        # This test may need adjustment based on actual behavior
        assert not comparison.matches or comparison.diff_percentage > 0, (
            "Mercator and Simple shaders produced identical output - unexpected"
        )

    def test_debug_shader_produces_output(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Debug shader should produce visible output."""
        pbf_file = test_data_dir / "cross_pattern.osm.pbf"
        output = temp_output_dir / "debug.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 11, 1024, 1024, output, shader_type="debug")

        assert result.success

        analyzer = ImageAnalyzer(output)

        # Debug shader should produce some output
        assert analyzer.non_white_count() > 0, "Debug shader produced no visible output"


@pytest.mark.unit
class TestPixelCounting:
    """Test pixel counting accuracy."""

    def test_pixel_count_matches_analysis(
        self, render_tile_binary, test_data_dir, temp_output_dir
    ):
        """Verify that RenderTileRunner's pixel count matches ImageAnalyzer."""
        pbf_file = test_data_dir / "horizontal_line.osm.pbf"
        output = temp_output_dir / "count_test.png"

        if not pbf_file.exists():
            pytest.skip(f"Test data not found: {pbf_file}")

        runner = RenderTileRunner(render_tile_binary)
        result = runner.render(pbf_file, 10, 512, 512, output)

        assert result.success

        # Get pixel count from renderer output
        renderer_non_white = result.non_white_pixels
        renderer_total = result.total_pixels

        # Get pixel count from image analysis
        analyzer = ImageAnalyzer(output)
        analyzer_non_white = analyzer.non_white_count()
        analyzer_total = analyzer.total_pixels

        # Should match (or be very close)
        assert renderer_total == analyzer_total, "Total pixel count mismatch"
        assert renderer_non_white == analyzer_non_white, (
            f"Non-white pixel count mismatch: "
            f"renderer={renderer_non_white}, analyzer={analyzer_non_white}"
        )
