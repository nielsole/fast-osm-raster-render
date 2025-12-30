#!/usr/bin/env python3
"""Update golden reference images for regression tests.

This script renders all test cases and saves them as golden images.
Use this when you intentionally change rendering behavior.
"""

import sys
from pathlib import Path

# Add parent directory to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from utils.renderer import find_render_tile_binary, RenderTileRunner
from test_regression import TEST_CASES


def main():
    """Render all test cases and save as golden images."""
    # Setup paths
    project_root = Path(__file__).parent.parent
    test_data_dir = project_root / "test_data" / "fixtures"
    golden_dir = project_root / "golden_images"
    temp_dir = project_root / "temp_renders"

    # Create directories
    golden_dir.mkdir(parents=True, exist_ok=True)
    temp_dir.mkdir(parents=True, exist_ok=True)

    # Find binary
    try:
        binary = find_render_tile_binary()
    except FileNotFoundError as e:
        print(f"Error: {e}")
        print("Run: cargo build --example render_tile")
        return 1

    print(f"Using render_tile: {binary}")
    print(f"Golden images directory: {golden_dir}")
    print()

    runner = RenderTileRunner(binary)
    shaders = ["mercator"]  # Match test_regression.py parametrization

    success_count = 0
    fail_count = 0

    for fixture_name, z, x, y, description in TEST_CASES:
        for shader in shaders:
            pbf_file = test_data_dir / f"{fixture_name}.osm.pbf"
            golden_path = golden_dir / f"{fixture_name}_z{z}_x{x}_y{y}_{shader}.png"
            temp_path = temp_dir / f"{fixture_name}_{shader}.png"

            if not pbf_file.exists():
                print(f"⚠️  SKIP: {fixture_name} - PBF not found")
                continue

            print(f"Rendering: {fixture_name} ({description}) - {shader}")

            # Render
            result = runner.render(pbf_file, z, x, y, temp_path, shader)

            if not result.success:
                print(f"  ❌ FAILED")
                print(f"     {result.stderr[:200]}")
                fail_count += 1
                continue

            # Move to golden directory
            import shutil
            shutil.copy2(temp_path, golden_path)

            print(f"  ✅ Saved: {golden_path.name}")
            print(f"     Pixels: {result.non_white_pixels:,} / {result.total_pixels:,}")
            print(f"     Time: {result.render_time:.3f}s")
            success_count += 1

    # Cleanup temp directory
    import shutil
    shutil.rmtree(temp_dir, ignore_errors=True)

    print()
    print("=" * 60)
    print(f"Generated {success_count} golden images")
    if fail_count > 0:
        print(f"Failed: {fail_count}")
        return 1

    print()
    print("Next steps:")
    print("  1. Review golden images:")
    print(f"     ls -lh {golden_dir}")
    print("  2. Commit golden images:")
    print("     git add golden_images/")
    print('     git commit -m "Add/update golden images"')
    print("  3. Run regression tests:")
    print("     pytest -m regression -v")

    return 0


if __name__ == "__main__":
    sys.exit(main())
