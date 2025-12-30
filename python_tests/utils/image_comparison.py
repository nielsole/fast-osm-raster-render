"""Image comparison utilities for regression testing."""

from PIL import Image
import numpy as np
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Dict


@dataclass
class ImageComparisonResult:
    """Result of comparing two images."""
    matches: bool
    diff_pixels: int
    diff_percentage: float
    diff_image_path: Optional[Path] = None


class ImageAnalyzer:
    """Analyze image properties for threshold-based testing."""

    def __init__(self, image_path: Path):
        """
        Initialize analyzer with an image.

        Args:
            image_path: Path to the image file
        """
        self.image_path = Path(image_path)
        self.image = Image.open(image_path).convert('RGB')
        self.array = np.array(self.image)
        self.height, self.width, _ = self.array.shape
        self.total_pixels = self.height * self.width

    def non_white_percentage(self) -> float:
        """
        Calculate percentage of non-white pixels.

        Returns:
            Percentage (0-100) of pixels that are not pure white (255,255,255)
        """
        white_mask = np.all(self.array == [255, 255, 255], axis=2)
        non_white_count = np.sum(~white_mask)
        return (non_white_count / self.total_pixels) * 100.0

    def non_white_count(self) -> int:
        """
        Count non-white pixels.

        Returns:
            Number of pixels that are not pure white
        """
        white_mask = np.all(self.array == [255, 255, 255], axis=2)
        return int(np.sum(~white_mask))

    def is_all_white(self) -> bool:
        """
        Check if the entire image is white.

        Returns:
            True if all pixels are (255,255,255)
        """
        return np.all(self.array == 255)

    def is_all_color(self, r: int, g: int, b: int, tolerance: int = 0) -> bool:
        """
        Check if the entire image is a specific color.

        Args:
            r, g, b: RGB values to check
            tolerance: Allowed deviation per channel

        Returns:
            True if all pixels match the color within tolerance
        """
        diff = np.abs(self.array.astype(int) - np.array([r, g, b]))
        return np.all(diff <= tolerance)

    def count_pixels_by_color(self, tolerance: int = 0) -> Dict[str, int]:
        """
        Count pixels by major categories.

        Args:
            tolerance: Allowed deviation for color matching

        Returns:
            Dictionary with counts: {'white': N, 'black': M, 'other': K}
        """
        counts = {}

        # Check for white (255,255,255)
        white_mask = np.all(np.abs(self.array - [255, 255, 255]) <= tolerance, axis=2)
        counts['white'] = int(np.sum(white_mask))

        # Check for black (0,0,0)
        black_mask = np.all(np.abs(self.array - [0, 0, 0]) <= tolerance, axis=2)
        counts['black'] = int(np.sum(black_mask))

        # Everything else
        counts['other'] = self.total_pixels - counts['white'] - counts['black']

        return counts

    def get_dominant_color(self) -> tuple[int, int, int]:
        """
        Get the most common RGB color in the image.

        Returns:
            (R, G, B) tuple of the most frequent color
        """
        # Reshape to list of pixels
        pixels = self.array.reshape(-1, 3)

        # Find unique colors and their counts
        unique_colors, counts = np.unique(pixels, axis=0, return_counts=True)

        # Get most common
        dominant_idx = np.argmax(counts)
        return tuple(unique_colors[dominant_idx])

    def color_coverage(self, r: int, g: int, b: int, tolerance: int = 0) -> float:
        """
        Calculate percentage of pixels within tolerance of a color.

        Args:
            r, g, b: Target RGB color
            tolerance: Allowed deviation per channel

        Returns:
            Percentage (0-100) of pixels matching the color
        """
        color_mask = np.all(np.abs(self.array - [r, g, b]) <= tolerance, axis=2)
        matching_count = np.sum(color_mask)
        return (matching_count / self.total_pixels) * 100.0


def compare_images_exact(
    image1_path: Path,
    image2_path: Path,
    tolerance: int = 0,
    save_diff: Optional[Path] = None,
) -> ImageComparisonResult:
    """
    Compare two images pixel-by-pixel.

    Args:
        image1_path: Path to first image
        image2_path: Path to second image
        tolerance: Allowed RGB difference per channel (0 = exact match)
        save_diff: Optional path to save visual diff image

    Returns:
        ImageComparisonResult with comparison details
    """
    # Load images as RGB
    img1 = np.array(Image.open(image1_path).convert('RGB'))
    img2 = np.array(Image.open(image2_path).convert('RGB'))

    # Check dimensions match
    if img1.shape != img2.shape:
        height1, width1, _ = img1.shape
        height2, width2, _ = img2.shape
        return ImageComparisonResult(
            matches=False,
            diff_pixels=-1,
            diff_percentage=100.0,
        )

    # Calculate per-pixel absolute difference
    diff = np.abs(img1.astype(int) - img2.astype(int))

    # Pixels that differ by more than tolerance in any channel
    diff_mask = np.any(diff > tolerance, axis=2)
    diff_pixels = int(np.sum(diff_mask))

    total_pixels = img1.shape[0] * img1.shape[1]
    diff_percentage = (diff_pixels / total_pixels) * 100.0

    matches = diff_pixels == 0

    # Optionally save visual diff
    diff_image_path = None
    if save_diff and diff_pixels > 0:
        # Create diff image: red where different, original where same
        diff_visual = img1.copy()
        diff_visual[diff_mask] = [255, 0, 0]  # Red for differences

        Image.fromarray(diff_visual.astype(np.uint8)).save(save_diff)
        diff_image_path = save_diff

    return ImageComparisonResult(
        matches=matches,
        diff_pixels=diff_pixels,
        diff_percentage=diff_percentage,
        diff_image_path=diff_image_path,
    )


def compare_images_hash(image1_path: Path, image2_path: Path) -> bool:
    """
    Fast comparison using file hash (byte-identical check).

    Args:
        image1_path: Path to first image
        image2_path: Path to second image

    Returns:
        True if images are byte-identical
    """
    import hashlib

    def file_hash(path: Path) -> str:
        """Compute SHA256 hash of file."""
        sha256 = hashlib.sha256()
        with open(path, 'rb') as f:
            while chunk := f.read(8192):
                sha256.update(chunk)
        return sha256.hexdigest()

    return file_hash(image1_path) == file_hash(image2_path)
