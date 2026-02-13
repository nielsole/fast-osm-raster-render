use super::serialization::{BOUNDING_BOX_SIZE, POINT_SIZE, POINTS_LEN_SIZE};
use super::types::{BoundingBox, MapObjectOffset, Point};
use memmap2::Mmap;
use std::collections::HashMap;
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
    pub fn read_map_object(&self, offset: MapObjectOffset) -> MapObjectView<'_> {
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

/// Zero-copy view into a map object in the memory-mapped file (v2 format)
///
/// # Safety
/// This structure contains references to memory-mapped data.
/// The data must not be modified externally while this view exists.
/// The MappedData must outlive all MapObjectView instances.
///
/// The v2 format uses 8-byte aligned flags field to preserve Point array alignment.
#[derive(Debug)]
pub struct MapObjectView<'a> {
    pub bbox: BoundingBox,
    points: &'a [Point],  // Zero-copy reference into mmap
    is_area: bool,
    tags_ptr: *const u8, // Pointer to start of tag data (num_tags u16 + key/value pairs)
}

const FLAGS_SIZE: usize = 8; // u64 flags, padded for alignment

impl<'a> MapObjectView<'a> {
    /// Create a MapObjectView from a raw pointer (v2 format only)
    ///
    /// # Safety
    /// The pointer must point to valid map object data in v2 format:
    /// - 8 bytes: version (u64 = 2)
    /// - 32 bytes: BoundingBox
    /// - 8 bytes: flags (u64, bit 0 = is_area, padded for alignment)
    /// - 8 bytes: i64 length
    /// - length * 16 bytes: Point array (aligned)
    /// - 2 bytes: num_tags (u16)
    /// - for each tag: 2 (key_len) + key + 2 (value_len) + value
    ///
    /// The memory must remain valid and unchanged for the lifetime 'a.
    unsafe fn from_ptr(ptr: *const u8) -> Self {
        use super::serialization::VERSION_SIZE;

        // Skip version (8 bytes)
        let data_ptr = ptr.add(VERSION_SIZE);

        // Read bounding box (4 f64s)
        let min_lon = data_ptr.cast::<f64>().read_unaligned();
        let min_lat = data_ptr.add(8).cast::<f64>().read_unaligned();
        let max_lon = data_ptr.add(16).cast::<f64>().read_unaligned();
        let max_lat = data_ptr.add(24).cast::<f64>().read_unaligned();

        let bbox = BoundingBox {
            min: Point::new(min_lon, min_lat),
            max: Point::new(max_lon, max_lat),
        };

        // Read flags (8 bytes after bbox, u64 for alignment)
        let flags_ptr = data_ptr.add(BOUNDING_BOX_SIZE);
        let flags = flags_ptr.cast::<u64>().read_unaligned();
        let is_area = (flags & 1) != 0;

        // Read points length (8 bytes after flags)
        let points_len_ptr = flags_ptr.add(FLAGS_SIZE);
        let points_len = points_len_ptr.cast::<i64>().read_unaligned() as usize;

        // Zero-copy points array (properly aligned since all preceding fields are 8-byte)
        let points_start = points_len_ptr.add(POINTS_LEN_SIZE);
        let points = std::slice::from_raw_parts(
            points_start as *const Point,
            points_len,
        );

        // Tags start after the points array
        let tags_ptr = points_start.add(points_len * POINT_SIZE);

        MapObjectView { bbox, points, is_area, tags_ptr }
    }

    /// Get the bounding box
    pub fn bounding_box(&self) -> &BoundingBox {
        &self.bbox
    }

    /// Get the points slice
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// Get the number of points
    pub fn num_points(&self) -> usize {
        self.points.len()
    }

    /// Check if this object is an area (closed polygon)
    pub fn is_area(&self) -> bool {
        self.is_area
    }

    /// Read all tags from memory-mapped data as a HashMap
    pub fn tags(&self) -> HashMap<String, String> {
        unsafe {
            let num_tags = self.tags_ptr.cast::<u16>().read_unaligned() as usize;
            let mut map = HashMap::with_capacity(num_tags);
            let mut cursor = self.tags_ptr.add(2); // skip num_tags

            for _ in 0..num_tags {
                let key_len = cursor.cast::<u16>().read_unaligned() as usize;
                cursor = cursor.add(2);
                let key_bytes = std::slice::from_raw_parts(cursor, key_len);
                let key = String::from_utf8_lossy(key_bytes).into_owned();
                cursor = cursor.add(key_len);

                let value_len = cursor.cast::<u16>().read_unaligned() as usize;
                cursor = cursor.add(2);
                let value_bytes = std::slice::from_raw_parts(cursor, value_len);
                let value = String::from_utf8_lossy(value_bytes).into_owned();
                cursor = cursor.add(value_len);

                map.insert(key, value);
            }

            map
        }
    }

    /// Look up a single tag value by key, without allocating a HashMap.
    /// Returns None if the tag is not present.
    pub fn tag_value(&self, target_key: &str) -> Option<String> {
        unsafe {
            let num_tags = self.tags_ptr.cast::<u16>().read_unaligned() as usize;
            let mut cursor = self.tags_ptr.add(2);

            for _ in 0..num_tags {
                let key_len = cursor.cast::<u16>().read_unaligned() as usize;
                cursor = cursor.add(2);
                let key_bytes = std::slice::from_raw_parts(cursor, key_len);
                cursor = cursor.add(key_len);

                let value_len = cursor.cast::<u16>().read_unaligned() as usize;
                cursor = cursor.add(2);
                let value_bytes = std::slice::from_raw_parts(cursor, value_len);
                cursor = cursor.add(value_len);

                if key_bytes == target_key.as_bytes() {
                    return Some(String::from_utf8_lossy(value_bytes).into_owned());
                }
            }

            None
        }
    }

    /// Check if a tag key exists (without allocating)
    pub fn has_tag(&self, target_key: &str) -> bool {
        unsafe {
            let num_tags = self.tags_ptr.cast::<u16>().read_unaligned() as usize;
            let mut cursor = self.tags_ptr.add(2);

            for _ in 0..num_tags {
                let key_len = cursor.cast::<u16>().read_unaligned() as usize;
                cursor = cursor.add(2);
                let key_bytes = std::slice::from_raw_parts(cursor, key_len);
                cursor = cursor.add(key_len);

                let value_len = cursor.cast::<u16>().read_unaligned() as usize;
                cursor = cursor.add(2);
                cursor = cursor.add(value_len);

                if key_bytes == target_key.as_bytes() {
                    return true;
                }
            }

            false
        }
    }

    /// Get number of tags (without reading them)
    pub fn num_tags(&self) -> usize {
        unsafe { self.tags_ptr.cast::<u16>().read_unaligned() as usize }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::serialization::write_map_object;
    use crate::data::types::MapObject;
    use tempfile::NamedTempFile;

    #[test]
    fn test_mmap_read_v2() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        let obj1 = MapObject::new(
            BoundingBox {
                min: Point::new(10.0, 20.0),
                max: Point::new(30.0, 40.0),
            },
            vec![
                Point::new(15.0, 25.0),
                Point::new(20.0, 30.0),
            ],
            false,
            vec![("highway".to_string(), "primary".to_string())],
        );

        let obj2 = MapObject::new(
            BoundingBox {
                min: Point::new(50.0, 60.0),
                max: Point::new(70.0, 80.0),
            },
            vec![
                Point::new(55.0, 65.0),
                Point::new(60.0, 70.0),
                Point::new(65.0, 75.0),
                Point::new(55.0, 65.0), // closed
            ],
            true,
            vec![
                ("building".to_string(), "yes".to_string()),
                ("name".to_string(), "Test".to_string()),
            ],
        );

        // Write objects
        let offset1 = write_map_object(temp_file.as_file_mut(), &obj1)?;
        let offset2 = write_map_object(temp_file.as_file_mut(), &obj2)?;

        // Ensure data is flushed
        use std::io::Write;
        temp_file.as_file_mut().flush()?;

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
        assert!(!view1.is_area());
        let tags1 = view1.tags();
        assert_eq!(tags1.get("highway"), Some(&"primary".to_string()));

        // Read second object
        let view2 = mmap_data.read_map_object(offset2);
        assert_eq!(view2.bbox.min.lon, 50.0);
        assert_eq!(view2.bbox.min.lat, 60.0);
        assert_eq!(view2.num_points(), 4);
        assert!(view2.is_area());
        let tags2 = view2.tags();
        assert_eq!(tags2.get("building"), Some(&"yes".to_string()));
        assert_eq!(tags2.get("name"), Some(&"Test".to_string()));

        Ok(())
    }
}
