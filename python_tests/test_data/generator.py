"""Generate synthetic OSM PBF test data."""

from pathlib import Path
import sys

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))

from utils.osm_builder import (
    OSMBuilder,
    tile_to_bbox,
    create_cross_pattern,
    create_grid_pattern,
)


def generate_cross_pattern(output_dir: Path) -> None:
    """Generate cross pattern test fixture for tile 11/1024/1024."""
    print("Generating cross_pattern.osm.pbf...")
    builder = create_cross_pattern(1024, 1024, 11)
    output_path = output_dir / "cross_pattern.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_horizontal_line(output_dir: Path) -> None:
    """Generate horizontal line test fixture for tile 10/512/512."""
    print("Generating horizontal_line.osm.pbf...")
    builder = OSMBuilder()

    # Get tile bbox
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(512, 512, 10)
    center_lat = (lat_min + lat_max) / 2

    # Single horizontal line across center
    builder.add_line(center_lat, lon_min, center_lat, lon_max)

    output_path = output_dir / "horizontal_line.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_vertical_line(output_dir: Path) -> None:
    """Generate vertical line test fixture for tile 10/512/512."""
    print("Generating vertical_line.osm.pbf...")
    builder = OSMBuilder()

    # Get tile bbox
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(512, 512, 10)
    center_lon = (lon_min + lon_max) / 2

    # Single vertical line across center
    builder.add_line(lat_min, center_lon, lat_max, center_lon)

    output_path = output_dir / "vertical_line.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_diagonal_line(output_dir: Path) -> None:
    """Generate diagonal line test fixture for tile 11/1081/660."""
    print("Generating diagonal_line.osm.pbf...")
    builder = OSMBuilder()

    # Get tile bbox (Hamburg tile)
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(1081, 660, 11)

    # Diagonal from bottom-left to top-right
    builder.add_line(lat_min, lon_min, lat_max, lon_max)

    output_path = output_dir / "diagonal_line.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_grid_pattern(output_dir: Path) -> None:
    """Generate grid pattern test fixture for tile 12/2048/2048."""
    print("Generating grid_pattern.osm.pbf...")
    builder = create_grid_pattern(2048, 2048, 12, rows=5, cols=5)
    output_path = output_dir / "grid_pattern.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_empty_tile(output_dir: Path) -> None:
    """Generate empty tile test fixture (no ways in bbox)."""
    print("Generating empty_tile.osm.pbf...")
    builder = OSMBuilder()

    # Add a way far from tile 0/0/0 (in valid coordinate range but not visible)
    # Place way in middle of Pacific Ocean, far from tile 0/0/0
    builder.add_line(
        -40.0,  # South Pacific
        -150.0,
        -40.1,
        -150.1,
    )

    output_path = output_dir / "empty_tile.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_single_point(output_dir: Path) -> None:
    """Generate single point (degenerate line) test fixture."""
    print("Generating single_point.osm.pbf...")
    builder = OSMBuilder()

    # Get tile bbox for 5/16/10
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(16, 10, 5)
    center_lon = (lon_min + lon_max) / 2
    center_lat = (lat_min + lat_max) / 2

    # Create a "line" with two identical points (degenerate)
    builder.add_line(center_lat, center_lon, center_lat, center_lon)

    output_path = output_dir / "single_point.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_boundary_cross(output_dir: Path) -> None:
    """Generate ways crossing all 4 tile boundaries."""
    print("Generating boundary_cross.osm.pbf...")
    builder = OSMBuilder()

    # Get tile bbox for 10/512/512
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(512, 512, 10)
    center_lon = (lon_min + lon_max) / 2
    center_lat = (lat_min + lat_max) / 2

    # Extend beyond boundaries (50% larger)
    margin_lon = (lon_max - lon_min) * 0.5
    margin_lat = (lat_max - lat_min) * 0.5

    # Horizontal line crossing left and right boundaries
    builder.add_line(
        center_lat,
        lon_min - margin_lon,
        center_lat,
        lon_max + margin_lon,
    )

    # Vertical line crossing top and bottom boundaries
    builder.add_line(
        lat_min - margin_lat,
        center_lon,
        lat_max + margin_lat,
        center_lon,
    )

    output_path = output_dir / "boundary_cross.osm.pbf"
    builder.build_to_pbf(output_path, temp_dir=output_dir)
    print(f"  → {output_path}")


def generate_all_test_data(output_dir: Path) -> None:
    """
    Generate all test data fixtures.

    Args:
        output_dir: Directory to save PBF files (typically test_data/fixtures/)
    """
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Generating test data in {output_dir}\n")

    # Generate all fixtures
    generators = [
        generate_cross_pattern,
        generate_horizontal_line,
        generate_vertical_line,
        generate_diagonal_line,
        generate_grid_pattern,
        generate_empty_tile,
        generate_single_point,
        generate_boundary_cross,
    ]

    for generator in generators:
        try:
            generator(output_dir)
        except Exception as e:
            print(f"  ERROR: {e}")
            import traceback
            traceback.print_exc()

    print(f"\nGenerated {len(list(output_dir.glob('*.pbf')))} test fixtures")


if __name__ == "__main__":
    # Default output directory
    default_output = Path(__file__).parent / "fixtures"

    if len(sys.argv) > 1:
        output_dir = Path(sys.argv[1])
    else:
        output_dir = default_output

    generate_all_test_data(output_dir)
