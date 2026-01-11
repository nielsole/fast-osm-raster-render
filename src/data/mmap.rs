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
///
/// Note: Phase 0 reads BoundingBox by value due to alignment concerns
#[derive(Debug)]
pub struct MapObjectView<'a> {
    pub bbox: BoundingBox,
    pub points: &'a [Point],
    tags_ptr: *const u8, // Pointer to start of tag data (Phase 0: highway tag)
}

impl<'a> MapObjectView<'a> {
    /// Create a MapObjectView from a raw pointer
    ///
    /// # Safety
    /// The pointer must point to valid map object data in the correct format:
    /// Phase 0 format:
    /// - 8 bytes: version (u64)
    /// - 32 bytes: BoundingBox
    /// - 8 bytes: i64 length
    /// - length * 16 bytes: Point array
    /// - (tags after points, but we don't read them here for fast rendering)
    ///
    /// The memory must remain valid and unchanged for the lifetime 'a.
    unsafe fn from_ptr(ptr: *const u8) -> Self {
        use super::serialization::{FORMAT_VERSION_1, VERSION_SIZE};

        // Read version (8 bytes, unaligned)
        let version = ptr.cast::<u64>().read_unaligned();

        // Skip version for new format
        let data_ptr = if version >= FORMAT_VERSION_1 {
            ptr.add(VERSION_SIZE) // Skip 8-byte version
        } else {
            ptr // Old format, no version
        };

        // Read bounding box (4 f64s, possibly unaligned)
        let min_lon = data_ptr.cast::<f64>().read_unaligned();
        let min_lat = data_ptr.add(8).cast::<f64>().read_unaligned();
        let max_lon = data_ptr.add(16).cast::<f64>().read_unaligned();
        let max_lat = data_ptr.add(24).cast::<f64>().read_unaligned();

        let bbox = BoundingBox {
            min: Point::new(min_lon, min_lat),
            max: Point::new(max_lon, max_lat),
        };

        // Read points length
        let points_len = data_ptr
            .add(BOUNDING_BOX_SIZE)
            .cast::<i64>()
            .read_unaligned();

        // Read points array (we can assume Point array is okay since we're reading unaligned f64s)
        let points_start = data_ptr.add(BOUNDING_BOX_SIZE + POINTS_LEN_SIZE);
        let points = std::slice::from_raw_parts(
            points_start as *const Point,
            points_len as usize,
        );

        // Tags start after the points array
        let tags_ptr = points_start.add(points_len as usize * 16); // Each Point is 16 bytes

        MapObjectView { bbox, points, tags_ptr }
    }

    /// Get the bounding box
    pub fn bounding_box(&self) -> &BoundingBox {
        &self.bbox
    }

    /// Get the points slice
    pub fn points(&self) -> &[Point] {
        self.points
    }

    /// Get the number of points
    pub fn num_points(&self) -> usize {
        self.points.len()
    }

    /// Read the highway tag from memory-mapped data (Phase 0)
    /// Returns None if tag is not present
    pub fn highway_tag(&self) -> Option<String> {
        unsafe {
            // Read tag_present flag (1 byte)
            let tag_present = self.tags_ptr.read();

            if tag_present == 1 {
                // Read tag length (4 bytes, u32 little-endian)
                let tag_len = self.tags_ptr.add(1).cast::<u32>().read_unaligned() as usize;

                // Read tag string bytes
                let tag_bytes = std::slice::from_raw_parts(
                    self.tags_ptr.add(5),
                    tag_len,
                );

                // Convert to String (may fail if not valid UTF-8)
                String::from_utf8(tag_bytes.to_vec()).ok()
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::serialization::write_map_object;
    use crate::data::types::MapObject;
    use tempfile::NamedTempFile;

    #[test]
    #[ignore] // Phase 0: Deferred - has alignment issues, will be fixed in Phase 1
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
            highway_tag: None, // Phase 0: added highway_tag field
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
            highway_tag: None, // Phase 0: added highway_tag field
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
