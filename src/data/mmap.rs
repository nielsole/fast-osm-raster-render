use super::serialization::{BOUNDING_BOX_SIZE, POINT_SIZE, POINTS_LEN_SIZE};
use super::types::{BoundingBox, MapObjectOffset, Point};
use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::Path;

/// Memory-mapped file for zero-copy access to map objects
pub struct MappedData {
    _file: File, // Keep file open for the lifetime of the mmap
    mmap: Mmap,
}

impl MappedData {
    /// Create a new memory-mapped file
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(MappedData { _file: file, mmap })
    }

    /// Get a zero-copy view of a map object at the given offset
    pub fn read_map_object(&self, offset: MapObjectOffset) -> MapObjectView {
        unsafe { MapObjectView::from_ptr(self.mmap.as_ptr().add(offset as usize)) }
    }

    /// Get the size of the memory-mapped region
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Check if the memory-mapped region is empty
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }
}

/// Zero-copy view into a map object in the memory-mapped file
///
/// # Safety
/// This structure contains references to memory-mapped data.
/// The data must not be modified externally while this view exists.
/// The MappedData must outlive all MapObjectView instances.
#[derive(Debug)]
pub struct MapObjectView<'a> {
    pub bbox: &'a BoundingBox,
    pub points: &'a [Point],
}

impl<'a> MapObjectView<'a> {
    /// Create a MapObjectView from a raw pointer
    ///
    /// # Safety
    /// The pointer must point to valid map object data in the correct format:
    /// - 32 bytes: BoundingBox
    /// - 8 bytes: i64 length
    /// - length * 16 bytes: Point array
    ///
    /// The memory must remain valid and unchanged for the lifetime 'a.
    unsafe fn from_ptr(ptr: *const u8) -> Self {
        // Read bounding box (first 32 bytes)
        let bbox = &*(ptr as *const BoundingBox);

        // Read points length (bytes 32-40)
        let points_len = *(ptr.add(BOUNDING_BOX_SIZE) as *const i64);

        // Read points array (starting at byte 40)
        let points = std::slice::from_raw_parts(
            ptr.add(BOUNDING_BOX_SIZE + POINTS_LEN_SIZE) as *const Point,
            points_len as usize,
        );

        MapObjectView { bbox, points }
    }

    /// Get the bounding box
    pub fn bounding_box(&self) -> &BoundingBox {
        self.bbox
    }

    /// Get the points slice
    pub fn points(&self) -> &[Point] {
        self.points
    }

    /// Get the number of points
    pub fn num_points(&self) -> usize {
        self.points.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::serialization::write_map_object;
    use crate::data::types::MapObject;
    use tempfile::NamedTempFile;

    #[test]
    fn test_mmap_read() -> io::Result<()> {
        // Create a temporary file with map objects
        let mut temp_file = NamedTempFile::new()?;

        let obj1 = MapObject {
            bounding_box: BoundingBox {
                min: Point::new(10.0, 20.0),
                max: Point::new(30.0, 40.0),
            },
            points: vec![
                Point::new(15.0, 25.0),
                Point::new(20.0, 30.0),
            ],
        };

        let obj2 = MapObject {
            bounding_box: BoundingBox {
                min: Point::new(50.0, 60.0),
                max: Point::new(70.0, 80.0),
            },
            points: vec![
                Point::new(55.0, 65.0),
                Point::new(60.0, 70.0),
                Point::new(65.0, 75.0),
            ],
        };

        // Write objects
        let offset1 = write_map_object(temp_file.as_file_mut(), &obj1)?;
        let offset2 = write_map_object(temp_file.as_file_mut(), &obj2)?;

        // Ensure data is flushed
        temp_file.as_file_mut().sync_all()?;

        // Memory map the file
        let mmap_data = MappedData::new(temp_file.path())?;

        // Read first object
        let view1 = mmap_data.read_map_object(offset1);
        assert_eq!(view1.bbox.min.lon, 10.0);
        assert_eq!(view1.bbox.min.lat, 20.0);
        assert_eq!(view1.bbox.max.lon, 30.0);
        assert_eq!(view1.bbox.max.lat, 40.0);
        assert_eq!(view1.num_points(), 2);
        assert_eq!(view1.points[0].lon, 15.0);
        assert_eq!(view1.points[0].lat, 25.0);

        // Read second object
        let view2 = mmap_data.read_map_object(offset2);
        assert_eq!(view2.bbox.min.lon, 50.0);
        assert_eq!(view2.bbox.min.lat, 60.0);
        assert_eq!(view2.num_points(), 3);
        assert_eq!(view2.points[1].lon, 60.0);
        assert_eq!(view2.points[1].lat, 70.0);

        Ok(())
    }
}
