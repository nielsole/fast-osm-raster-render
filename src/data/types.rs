use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl Tile {
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        Tile { x, y, z }
    }

    /// Calculate tile index in quadtree
    /// This MUST match the Go version exactly
    pub fn index(&self) -> u64 {
        // Calculate the total number of tiles for all zoom levels from 0 to z-1
        let mut total = 0u64;
        for z in 0..self.z {
            total += 4u64.pow(z);
        }

        // Calculate the position of the tile within its zoom level
        let level_pos = self.y * 2u32.pow(self.z) + self.x;

        total + level_pos as u64
    }

    /// Get the parent tile (one zoom level up)
    pub fn get_parent(&self) -> Option<Tile> {
        if self.z == 0 {
            None
        } else {
            Some(Tile {
                x: self.x / 2,
                y: self.y / 2,
                z: self.z - 1,
            })
        }
    }

    /// Get ancestor tile at a specific zoom level
    /// Returns None if target_z > self.z
    pub fn get_ancestor(&self, target_z: u32) -> Option<Tile> {
        if target_z > self.z {
            return None;
        }
        if target_z == self.z {
            return Some(*self);
        }

        // Calculate how many zoom levels to go up
        let levels_up = self.z - target_z;
        Some(Tile {
            x: self.x >> levels_up,
            y: self.y >> levels_up,
            z: target_z,
        })
    }
}

impl fmt::Display for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Point {
    pub lon: f64,
    pub lat: f64,
}

impl Point {
    pub fn new(lon: f64, lat: f64) -> Self {
        Point { lon, lat }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct BoundingBox {
    pub min: Point,
    pub max: Point,
}

impl BoundingBox {
    pub fn new(min: Point, max: Point) -> Self {
        BoundingBox { min, max }
    }

    /// Check if a point is inside this bounding box
    pub fn contains(&self, point: &Point) -> bool {
        point.lat >= self.min.lat
            && point.lat <= self.max.lat
            && point.lon >= self.min.lon
            && point.lon <= self.max.lon
    }

    /// Check if this bounding box overlaps with another
    pub fn overlaps(&self, other: &BoundingBox) -> bool {
        self.min.lat <= other.max.lat
            && self.max.lat >= other.min.lat
            && self.min.lon <= other.max.lon
            && self.max.lon >= other.min.lon
    }

    /// Get the center point of this bounding box
    pub fn center(&self) -> Point {
        Point {
            lon: (self.min.lon + self.max.lon) / 2.0,
            lat: (self.min.lat + self.max.lat) / 2.0,
        }
    }

    /// Create bounding box from a list of points
    pub fn from_points(points: &[Point]) -> Option<Self> {
        if points.is_empty() {
            return None;
        }

        let mut min_lon = f64::MAX;
        let mut min_lat = f64::MAX;
        let mut max_lon = f64::MIN;
        let mut max_lat = f64::MIN;

        for point in points {
            min_lon = min_lon.min(point.lon);
            min_lat = min_lat.min(point.lat);
            max_lon = max_lon.max(point.lon);
            max_lat = max_lat.max(point.lat);
        }

        Some(BoundingBox {
            min: Point::new(min_lon, min_lat),
            max: Point::new(max_lon, max_lat),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    pub x: f64,
    pub y: f64,
}

/// Offset into the memory-mapped file
pub type MapObjectOffset = u64;

/// Map object representing a way from OSM
#[derive(Debug, Clone)]
pub struct MapObject {
    pub bounding_box: BoundingBox,
    pub points: Vec<Point>,
}

impl MapObject {
    pub fn new(bounding_box: BoundingBox, points: Vec<Point>) -> Self {
        MapObject {
            bounding_box,
            points,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_index() {
        // Test cases that must match Go version
        assert_eq!(Tile::new(0, 0, 0).index(), 0);
        assert_eq!(Tile::new(0, 0, 1).index(), 1);
        assert_eq!(Tile::new(1, 0, 1).index(), 2);
        assert_eq!(Tile::new(0, 1, 1).index(), 3);
        assert_eq!(Tile::new(1, 1, 1).index(), 4);

        // Zoom level 2 starts at index 5 (after 1 + 4 tiles)
        assert_eq!(Tile::new(0, 0, 2).index(), 5);
    }

    #[test]
    fn test_tile_parent() {
        let tile = Tile::new(4, 6, 3);
        let parent = tile.get_parent().unwrap();
        assert_eq!(parent, Tile::new(2, 3, 2));

        let root = Tile::new(0, 0, 0);
        assert_eq!(root.get_parent(), None);
    }

    #[test]
    fn test_tile_ancestor() {
        // Test getting ancestor at lower zoom
        let tile = Tile::new(100, 200, 17);
        let ancestor = tile.get_ancestor(15).unwrap();
        // 100 >> 2 = 25, 200 >> 2 = 50
        assert_eq!(ancestor, Tile::new(25, 50, 15));

        // Test getting self
        let tile = Tile::new(10, 20, 15);
        let ancestor = tile.get_ancestor(15).unwrap();
        assert_eq!(ancestor, tile);

        // Test invalid (target_z > self.z)
        let tile = Tile::new(10, 20, 15);
        assert_eq!(tile.get_ancestor(16), None);

        // Test multiple levels
        let tile = Tile::new(64, 128, 18);
        let ancestor = tile.get_ancestor(15).unwrap();
        // 64 >> 3 = 8, 128 >> 3 = 16
        assert_eq!(ancestor, Tile::new(8, 16, 15));
    }

    #[test]
    fn test_bounding_box_contains() {
        let bbox = BoundingBox::new(
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
        );

        assert!(bbox.contains(&Point::new(15.0, 25.0)));
        assert!(!bbox.contains(&Point::new(5.0, 25.0)));
        assert!(!bbox.contains(&Point::new(15.0, 50.0)));
    }

    #[test]
    fn test_bounding_box_overlaps() {
        let bbox1 = BoundingBox::new(
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
        );
        let bbox2 = BoundingBox::new(
            Point::new(25.0, 35.0),
            Point::new(45.0, 55.0),
        );
        let bbox3 = BoundingBox::new(
            Point::new(50.0, 60.0),
            Point::new(70.0, 80.0),
        );

        assert!(bbox1.overlaps(&bbox2));
        assert!(bbox2.overlaps(&bbox1));
        assert!(!bbox1.overlaps(&bbox3));
    }

    #[test]
    fn test_bounding_box_from_points() {
        let points = vec![
            Point::new(10.0, 20.0),
            Point::new(30.0, 40.0),
            Point::new(15.0, 25.0),
        ];

        let bbox = BoundingBox::from_points(&points).unwrap();
        assert_eq!(bbox.min.lon, 10.0);
        assert_eq!(bbox.min.lat, 20.0);
        assert_eq!(bbox.max.lon, 30.0);
        assert_eq!(bbox.max.lat, 40.0);
    }
}
