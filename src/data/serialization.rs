use super::types::{BoundingBox, MapObject, MapObjectOffset, Point};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

/// Binary format v2:
/// - version: 8 bytes (u64 = 2)
/// - BoundingBox: 32 bytes (4 x f64)
/// - flags: 8 bytes (u64, bit 0 = is_area; padded for alignment)
/// - points_len: 8 bytes (i64)
/// - points: points_len * 16 bytes (f64 lon + f64 lat)
/// - num_tags: 2 bytes (u16)
/// - for each tag:
///   - key_len: 2 bytes (u16)
///   - key: key_len bytes (UTF-8)
///   - value_len: 2 bytes (u16)
///   - value: value_len bytes (UTF-8)

pub const VERSION_SIZE: usize = 8;
pub const BOUNDING_BOX_SIZE: usize = 32;
pub const POINTS_LEN_SIZE: usize = 8;
pub const POINT_SIZE: usize = 16;

/// Binary format version
pub const FORMAT_VERSION: u64 = 2;

/// Write a map object to a writer and return its offset
pub fn write_map_object<W: WriteBytesExt + Seek>(writer: &mut W, obj: &MapObject) -> io::Result<MapObjectOffset> {
    let offset = writer.stream_position()?;

    // Write version (8 bytes for alignment)
    writer.write_u64::<LittleEndian>(FORMAT_VERSION)?;

    // Write bounding box (32 bytes)
    writer.write_f64::<LittleEndian>(obj.bounding_box.min.lon)?;
    writer.write_f64::<LittleEndian>(obj.bounding_box.min.lat)?;
    writer.write_f64::<LittleEndian>(obj.bounding_box.max.lon)?;
    writer.write_f64::<LittleEndian>(obj.bounding_box.max.lat)?;

    // Write flags (8 bytes, padded for alignment)
    let flags: u64 = if obj.is_area { 1 } else { 0 };
    writer.write_u64::<LittleEndian>(flags)?;

    // Write points length (8 bytes)
    writer.write_i64::<LittleEndian>(obj.points.len() as i64)?;

    // Write points
    for point in &obj.points {
        writer.write_f64::<LittleEndian>(point.lon)?;
        writer.write_f64::<LittleEndian>(point.lat)?;
    }

    // Write tags
    writer.write_u16::<LittleEndian>(obj.tags.len() as u16)?;
    for (key, value) in &obj.tags {
        writer.write_u16::<LittleEndian>(key.len() as u16)?;
        writer.write_all(key.as_bytes())?;
        writer.write_u16::<LittleEndian>(value.len() as u16)?;
        writer.write_all(value.as_bytes())?;
    }

    // Pad to 8-byte alignment so the next object starts aligned
    let pos = writer.stream_position()?;
    let padding = (8 - (pos % 8)) % 8;
    for _ in 0..padding {
        writer.write_u8(0)?;
    }

    Ok(offset)
}

/// Read a map object from a file at a given offset
pub fn read_map_object(file: &mut File, offset: MapObjectOffset) -> io::Result<MapObject> {
    file.seek(SeekFrom::Start(offset))?;

    // Read version (8 bytes)
    let version = file.read_u64::<LittleEndian>()?;
    if version != FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Unsupported format version: {} (expected {})", version, FORMAT_VERSION),
        ));
    }

    // Read bounding box (32 bytes)
    let min_lon = file.read_f64::<LittleEndian>()?;
    let min_lat = file.read_f64::<LittleEndian>()?;
    let max_lon = file.read_f64::<LittleEndian>()?;
    let max_lat = file.read_f64::<LittleEndian>()?;

    let bounding_box = BoundingBox {
        min: Point::new(min_lon, min_lat),
        max: Point::new(max_lon, max_lat),
    };

    // Read flags (8 bytes, padded for alignment)
    let flags = file.read_u64::<LittleEndian>()?;
    let is_area = (flags & 1) != 0;

    // Read points length (8 bytes)
    let points_len = file.read_i64::<LittleEndian>()?;

    // Read points
    let mut points = Vec::with_capacity(points_len as usize);
    for _ in 0..points_len {
        let lon = file.read_f64::<LittleEndian>()?;
        let lat = file.read_f64::<LittleEndian>()?;
        points.push(Point::new(lon, lat));
    }

    // Read tags
    let num_tags = file.read_u16::<LittleEndian>()? as usize;
    let mut tags = Vec::with_capacity(num_tags);
    for _ in 0..num_tags {
        let key_len = file.read_u16::<LittleEndian>()? as usize;
        let mut key_bytes = vec![0u8; key_len];
        file.read_exact(&mut key_bytes)?;
        let key = String::from_utf8(key_bytes).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8 key: {}", e))
        })?;

        let value_len = file.read_u16::<LittleEndian>()? as usize;
        let mut value_bytes = vec![0u8; value_len];
        file.read_exact(&mut value_bytes)?;
        let value = String::from_utf8(value_bytes).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8 value: {}", e))
        })?;

        tags.push((key, value));
    }

    Ok(MapObject {
        bounding_box,
        points,
        is_area,
        tags,
    })
}

/// Calculate the size of a map object in bytes (approximate, without tags)
pub fn map_object_size(num_points: usize) -> usize {
    BOUNDING_BOX_SIZE + POINTS_LEN_SIZE + (num_points * POINT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::NamedTempFile;

    #[test]
    fn test_write_and_read_map_object_no_tags() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        let original = MapObject::new(
            BoundingBox {
                min: Point::new(10.0, 20.0),
                max: Point::new(30.0, 40.0),
            },
            vec![
                Point::new(15.0, 25.0),
                Point::new(20.0, 30.0),
                Point::new(25.0, 35.0),
            ],
            false,
            vec![],
        );

        // Write
        let offset = write_map_object(temp_file.as_file_mut(), &original)?;
        assert_eq!(offset, 0);

        // Read
        let read_obj = read_map_object(temp_file.as_file_mut(), offset)?;

        // Verify
        assert_eq!(read_obj.bounding_box.min.lon, original.bounding_box.min.lon);
        assert_eq!(read_obj.bounding_box.min.lat, original.bounding_box.min.lat);
        assert_eq!(read_obj.bounding_box.max.lon, original.bounding_box.max.lon);
        assert_eq!(read_obj.bounding_box.max.lat, original.bounding_box.max.lat);
        assert_eq!(read_obj.points.len(), original.points.len());
        for (i, point) in read_obj.points.iter().enumerate() {
            assert_eq!(point.lon, original.points[i].lon);
            assert_eq!(point.lat, original.points[i].lat);
        }
        assert!(!read_obj.is_area);
        assert!(read_obj.tags.is_empty());

        Ok(())
    }

    #[test]
    fn test_write_and_read_map_object_with_tags() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        let original = MapObject::new(
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

        // Write
        let offset = write_map_object(temp_file.as_file_mut(), &original)?;

        // Read
        let read_obj = read_map_object(temp_file.as_file_mut(), offset)?;

        // Verify geometry
        assert_eq!(read_obj.bounding_box.min.lon, 10.0);
        assert_eq!(read_obj.points.len(), 2);

        // Verify tags
        assert_eq!(read_obj.tags.len(), 1);
        assert_eq!(read_obj.tags[0], ("highway".to_string(), "primary".to_string()));
        assert!(!read_obj.is_area);

        Ok(())
    }

    #[test]
    fn test_write_and_read_area_object() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        let original = MapObject::new(
            BoundingBox {
                min: Point::new(1.0, 2.0),
                max: Point::new(3.0, 4.0),
            },
            vec![
                Point::new(1.0, 2.0),
                Point::new(3.0, 2.0),
                Point::new(3.0, 4.0),
                Point::new(1.0, 4.0),
                Point::new(1.0, 2.0), // closed
            ],
            true,
            vec![
                ("building".to_string(), "yes".to_string()),
                ("name".to_string(), "Test".to_string()),
            ],
        );

        let offset = write_map_object(temp_file.as_file_mut(), &original)?;
        let read_obj = read_map_object(temp_file.as_file_mut(), offset)?;

        assert!(read_obj.is_area);
        assert_eq!(read_obj.points.len(), 5);
        assert_eq!(read_obj.tags.len(), 2);
        assert_eq!(read_obj.tags[0], ("building".to_string(), "yes".to_string()));
        assert_eq!(read_obj.tags[1], ("name".to_string(), "Test".to_string()));

        Ok(())
    }

    #[test]
    fn test_binary_format_v2() -> io::Result<()> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);

        let obj = MapObject::new(
            BoundingBox {
                min: Point::new(1.0, 2.0),
                max: Point::new(3.0, 4.0),
            },
            vec![Point::new(5.0, 6.0)],
            true,
            vec![("highway".to_string(), "motorway".to_string())],
        );

        write_map_object(&mut cursor, &obj)?;

        // Check format:
        // 8 (version) + 32 (bbox) + 8 (flags) + 8 (len) + 16 (point) + 2 (num_tags)
        // + 2 (key_len) + 7 (highway) + 2 (value_len) + 8 (motorway) = 93
        // + 3 padding bytes to reach 96 (8-byte aligned)
        assert_eq!(buffer.len(), 96);
        assert_eq!(buffer.len() % 8, 0); // Verify 8-byte alignment

        // Verify version (first 8 bytes as u64)
        let mut cursor = Cursor::new(&buffer);
        let version = cursor.read_u64::<LittleEndian>()?;
        assert_eq!(version, FORMAT_VERSION);

        Ok(())
    }

    #[test]
    fn test_map_object_size() {
        assert_eq!(map_object_size(0), 40); // 32 + 8 + 0
        assert_eq!(map_object_size(1), 56); // 32 + 8 + 16
        assert_eq!(map_object_size(10), 200); // 32 + 8 + 160
    }
}
