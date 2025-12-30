"""OSM data builder for creating synthetic test fixtures."""

import subprocess
import math
from pathlib import Path
from typing import List, Tuple, Dict, Optional
import xml.etree.ElementTree as ET


def tile_to_bbox(x: int, y: int, z: int) -> Tuple[float, float, float, float]:
    """
    Convert tile coordinates to lat/lon bounding box using Web Mercator.

    Args:
        x: Tile X coordinate
        y: Tile Y coordinate
        z: Zoom level

    Returns:
        (lon_min, lat_min, lon_max, lat_max)
    """
    n = 2 ** z

    lon_min = x / n * 360.0 - 180.0
    lon_max = (x + 1) / n * 360.0 - 180.0

    lat_min = math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * (y + 1) / n))))
    lat_max = math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * y / n))))

    return lon_min, lat_min, lon_max, lat_max


class OSMBuilder:
    """Build synthetic OSM XML data for testing."""

    def __init__(self):
        """Initialize an empty OSM dataset."""
        self.nodes: List[Tuple[int, float, float]] = []  # (id, lat, lon)
        self.ways: List[Tuple[int, List[int], Dict[str, str]]] = []  # (id, node_ids, tags)
        self.node_id = 1
        self.way_id = 1

    def add_node(self, lat: float, lon: float) -> int:
        """
        Add a node to the dataset.

        Args:
            lat: Latitude
            lon: Longitude

        Returns:
            Node ID
        """
        node_id = self.node_id
        self.nodes.append((node_id, lat, lon))
        self.node_id += 1
        return node_id

    def add_line(
        self,
        start_lat: float,
        start_lon: float,
        end_lat: float,
        end_lon: float,
        tags: Optional[Dict[str, str]] = None,
    ) -> int:
        """
        Add a simple 2-point line way.

        Args:
            start_lat: Starting latitude
            start_lon: Starting longitude
            end_lat: Ending latitude
            end_lon: Ending longitude
            tags: Optional tags for the way (default: highway=primary)

        Returns:
            Way ID
        """
        if tags is None:
            tags = {"highway": "primary"}

        # Create nodes
        n1 = self.add_node(start_lat, start_lon)
        n2 = self.add_node(end_lat, end_lon)

        # Create way
        return self._add_way([n1, n2], tags)

    def add_polyline(
        self,
        points: List[Tuple[float, float]],
        tags: Optional[Dict[str, str]] = None,
    ) -> int:
        """
        Add a multi-point way.

        Args:
            points: List of (lat, lon) tuples
            tags: Optional tags for the way (default: highway=primary)

        Returns:
            Way ID
        """
        if tags is None:
            tags = {"highway": "primary"}

        # Create nodes for each point
        node_ids = [self.add_node(lat, lon) for lat, lon in points]

        # Create way
        return self._add_way(node_ids, tags)

    def _add_way(self, node_ids: List[int], tags: Dict[str, str]) -> int:
        """
        Add a way with given node references.

        Args:
            node_ids: List of node IDs
            tags: Tags for the way

        Returns:
            Way ID
        """
        way_id = self.way_id
        self.ways.append((way_id, node_ids, tags))
        self.way_id += 1
        return way_id

    def build_to_xml(self, output_path: Path) -> None:
        """
        Generate OSM XML file.

        Args:
            output_path: Where to save the XML file
        """
        # Create root OSM element
        osm = ET.Element("osm", version="0.6", generator="OSMBuilder")

        # Add all nodes
        for node_id, lat, lon in self.nodes:
            ET.SubElement(
                osm,
                "node",
                id=str(node_id),
                lat=str(lat),
                lon=str(lon),
                version="1",
            )

        # Add all ways
        for way_id, node_ids, tags in self.ways:
            way_elem = ET.SubElement(osm, "way", id=str(way_id), version="1")

            # Add node references
            for node_id in node_ids:
                ET.SubElement(way_elem, "nd", ref=str(node_id))

            # Add tags
            for key, value in tags.items():
                ET.SubElement(way_elem, "tag", k=key, v=value)

        # Write to file with pretty formatting
        tree = ET.ElementTree(osm)
        ET.indent(tree, space="  ")
        tree.write(output_path, encoding="utf-8", xml_declaration=True)

    def build_to_pbf(
        self,
        output_path: Path,
        temp_dir: Optional[Path] = None,
        keep_xml: bool = False,
    ) -> None:
        """
        Generate OSM PBF file with embedded node locations.

        Uses osmium CLI tools to convert XML → PBF and add locations.

        Args:
            output_path: Where to save the final PBF file
            temp_dir: Optional directory for temporary files
            keep_xml: Keep the intermediate XML file

        Raises:
            FileNotFoundError: If osmium is not installed
            subprocess.CalledProcessError: If conversion fails
        """
        # Determine temp directory
        if temp_dir is None:
            temp_dir = output_path.parent

        temp_dir.mkdir(parents=True, exist_ok=True)

        # Generate intermediate files
        xml_path = temp_dir / f"{output_path.stem}_temp.osm.xml"
        pbf_temp_path = temp_dir / f"{output_path.stem}_temp.osm.pbf"

        try:
            # Step 1: Write OSM XML
            self.build_to_xml(xml_path)

            # Step 2: Convert XML to PBF
            subprocess.run(
                ["osmium", "cat", str(xml_path), "-o", str(pbf_temp_path)],
                check=True,
                capture_output=True,
                text=True,
            )

            # Step 3: Add node locations to ways
            subprocess.run(
                [
                    "osmium",
                    "add-locations-to-ways",
                    str(pbf_temp_path),
                    "-o",
                    str(output_path),
                ],
                check=True,
                capture_output=True,
                text=True,
            )

        except FileNotFoundError:
            raise FileNotFoundError(
                "osmium command not found. Install with: sudo apt-get install osmium-tool"
            )
        finally:
            # Cleanup temporary files
            if not keep_xml and xml_path.exists():
                xml_path.unlink()
            if pbf_temp_path.exists():
                pbf_temp_path.unlink()


def create_cross_pattern(tile_x: int, tile_y: int, tile_z: int) -> OSMBuilder:
    """
    Create a cross pattern (horizontal + vertical line) in a tile.

    Args:
        tile_x: Tile X coordinate
        tile_y: Tile Y coordinate
        tile_z: Zoom level

    Returns:
        OSMBuilder with cross pattern
    """
    builder = OSMBuilder()

    # Get tile bounding box
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(tile_x, tile_y, tile_z)

    # Calculate center and size
    center_lon = (lon_min + lon_max) / 2
    center_lat = (lat_min + lat_max) / 2
    size_lon = (lon_max - lon_min) * 0.4  # 80% of tile width
    size_lat = (lat_max - lat_min) * 0.4  # 80% of tile height

    # Horizontal line
    builder.add_line(
        center_lat,
        center_lon - size_lon,
        center_lat,
        center_lon + size_lon,
    )

    # Vertical line
    builder.add_line(
        center_lat - size_lat,
        center_lon,
        center_lat + size_lat,
        center_lon,
    )

    return builder


def create_grid_pattern(
    tile_x: int,
    tile_y: int,
    tile_z: int,
    rows: int = 5,
    cols: int = 5,
) -> OSMBuilder:
    """
    Create a grid pattern in a tile.

    Args:
        tile_x: Tile X coordinate
        tile_y: Tile Y coordinate
        tile_z: Zoom level
        rows: Number of horizontal lines
        cols: Number of vertical lines

    Returns:
        OSMBuilder with grid pattern
    """
    builder = OSMBuilder()

    # Get tile bounding box
    lon_min, lat_min, lon_max, lat_max = tile_to_bbox(tile_x, tile_y, tile_z)

    # Add horizontal lines
    for i in range(rows):
        lat = lat_min + (lat_max - lat_min) * (i + 1) / (rows + 1)
        builder.add_line(lat, lon_min, lat, lon_max)

    # Add vertical lines
    for i in range(cols):
        lon = lon_min + (lon_max - lon_min) * (i + 1) / (cols + 1)
        builder.add_line(lat_min, lon, lat_max, lon)

    return builder
