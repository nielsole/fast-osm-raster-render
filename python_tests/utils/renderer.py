"""Wrapper for executing the render_tile binary."""

import subprocess
import time
import re
from pathlib import Path
from dataclasses import dataclass
from typing import Optional


@dataclass
class RenderResult:
    """Result of rendering a tile."""
    success: bool
    output_path: Path
    stdout: str
    stderr: str
    render_time: float
    non_white_pixels: int
    total_pixels: int


class RenderTileRunner:
    """Execute the render_tile binary and parse results."""

    def __init__(self, binary_path: Path):
        """
        Initialize the runner with the path to render_tile binary.

        Args:
            binary_path: Path to the render_tile executable

        Raises:
            FileNotFoundError: If the binary doesn't exist
        """
        self.binary_path = Path(binary_path)
        if not self.binary_path.exists():
            raise FileNotFoundError(
                f"render_tile binary not found: {binary_path}\n"
                f"Run: cargo build --example render_tile"
            )

    def render(
        self,
        osm_file: Path,
        z: int,
        x: int,
        y: int,
        output_path: Path,
        shader_type: str = "mercator",
        timeout: int = 30,
    ) -> RenderResult:
        """
        Render a tile using the render_tile binary.

        Args:
            osm_file: Path to the OSM PBF file
            z: Zoom level
            x: Tile X coordinate
            y: Tile Y coordinate
            output_path: Where to save the output PNG
            shader_type: One of "mercator", "simple", "debug"
            timeout: Maximum seconds to wait for rendering

        Returns:
            RenderResult with execution details and pixel counts

        Raises:
            subprocess.TimeoutExpired: If rendering takes longer than timeout
        """
        # Build command
        cmd = [
            str(self.binary_path),
            str(osm_file),
            str(z),
            str(x),
            str(y),
            str(output_path),
        ]

        # Add shader flag if not default
        if shader_type == "simple":
            cmd.append("--simple-shader")
        elif shader_type == "debug":
            cmd.append("--debug-shader")
        # mercator is default, no flag needed

        # Execute and measure time
        # Run from project root so shader files can be found
        project_root = self.binary_path.parent.parent.parent.parent
        start = time.perf_counter()
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=project_root,
        )
        elapsed = time.perf_counter() - start

        # Parse output for pixel counts
        non_white, total = self._parse_pixel_counts(result.stdout)

        return RenderResult(
            success=result.returncode == 0,
            output_path=Path(output_path),
            stdout=result.stdout,
            stderr=result.stderr,
            render_time=elapsed,
            non_white_pixels=non_white,
            total_pixels=total,
        )

    def _parse_pixel_counts(self, stdout: str) -> tuple[int, int]:
        """
        Extract pixel counts from stdout.

        Looks for: "RESULT: 1523 non-white pixels / 65536 total (2.3%)"

        Returns:
            (non_white_pixels, total_pixels)
        """
        # Try to find the RESULT line
        pattern = r"RESULT:\s+(\d+)\s+non-white pixels\s+/\s+(\d+)\s+total"
        match = re.search(pattern, stdout)

        if match:
            non_white = int(match.group(1))
            total = int(match.group(2))
            return non_white, total

        # Default to full tile size if not found
        return 0, 256 * 256


def find_render_tile_binary() -> Path:
    """
    Locate the render_tile binary in the project.

    Searches in order:
    1. target/release/examples/render_tile
    2. target/debug/examples/render_tile

    Returns:
        Path to the binary

    Raises:
        FileNotFoundError: If binary is not found in any location
    """
    # Get project root (3 levels up from this file)
    project_root = Path(__file__).parent.parent.parent

    candidates = [
        project_root / "target" / "release" / "examples" / "render_tile",
        project_root / "target" / "debug" / "examples" / "render_tile",
    ]

    for path in candidates:
        if path.exists():
            return path

    raise FileNotFoundError(
        "render_tile binary not found.\n"
        "Searched:\n" + "\n".join(f"  - {p}" for p in candidates) + "\n"
        "Run: cargo build --example render_tile"
    )
