use super::types::{BoundingBox, MapObject, MapObjectOffset, Point};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

/// Binary format (Phase 0 - with highway tag):
/// - version: 8 bytes (u64) - 0 = old format, 1 = with highway tag
///   (8 bytes to maintain alignment for BoundingBox)
/// - BoundingBox: 32 bytes
///   - min.lon: 8 bytes (f64)
///   - min.lat: 8 bytes (f64)
///   - max.lon: 8 bytes (f64)
///   - max.lat: 8 bytes (f64)
/// - points_len: 8 bytes (i64)
/// - points: points_len * 16 bytes
///   - each point: lon (8 bytes f64) + lat (8 bytes f64)
/// - tag_present: 1 byte (0 = no tag, 1 = has highway tag)
/// - if tag_present:
///   - tag_len: 4 bytes (u32)
///   - tag_data: tag_len bytes (UTF-8 string)

pub const VERSION_SIZE: usize = 8;
pub const BOUNDING_BOX_SIZE: usize = 32;
pub const POINTS_LEN_SIZE: usize = 8;
pub const POINT_SIZE: usize = 16;

/// Binary format version
pub const FORMAT_VERSION_0: u64 = 0; // Original format (no tags)
pub const FORMAT_VERSION_1: u64 = 1; // Phase 0 format (highway tag)

/// Write a map object to a writer and return its offset (Phase 0: with highway tag)
pub fn write_map_object<W: WriteBytesExt + Seek>(writer: &mut W, obj: &MapObject) -> io::Result<MapObjectOffset> {
    let offset = writer.stream_position()?;

    // Write version (8 bytes for alignment)
    writer.write_u64::<LittleEndian>(FORMAT_VERSION_1)?;

    // Write bounding box (32 bytes)
    writer.write_f64::<LittleEndian>(obj.bounding_box.min.lon)?;
    writer.write_f64::<LittleEndian>(obj.bounding_box.min.lat)?;
    writer.write_f64::<LittleEndian>(obj.bounding_box.max.lon)?;
    writer.write_f64::<LittleEndian>(obj.bounding_box.max.lat)?;

    // Write length (8 bytes)
    writer.write_i64::<LittleEndian>(obj.points.len() as i64)?;

    // Write points
    for point in &obj.points {
        writer.write_f64::<LittleEndian>(point.lon)?;
        writer.write_f64::<LittleEndian>(point.lat)?;
    }

    // Write highway tag (Phase 0)
    if let Some(tag) = &obj.highway_tag {
        writer.write_u8(1)?; // tag_present = 1
        writer.write_u32::<LittleEndian>(tag.len() as u32)?;
        writer.write_all(tag.as_bytes())?;
    } else {
        writer.write_u8(0)?; // tag_present = 0
    }

    Ok(offset)
}

/// Read a map object from a file at a given offset (Phase 0: supports both formats)
pub fn read_map_object(file: &mut File, offset: MapObjectOffset) -> io::Result<MapObject> {
    file.seek(SeekFrom::Start(offset))?;

    // Read version (8 bytes)
    let version = file.read_u64::<LittleEndian>()?;

    // For old format (version 0), rewind and read without version byte
    // This maintains backward compatibility
    if version == FORMAT_VERSION_0 {
        // Treat this byte as part of bounding box min.lon
        // Seek back and read as old format
        file.seek(SeekFrom::Start(offset))?;
        return read_map_object_v0(file);
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

    // Read length (8 bytes)
    let points_len = file.read_i64::<LittleEndian>()?;

    // Read points
    let mut points = Vec::with_capacity(points_len as usize);
    for _ in 0..points_len {
        let lon = file.read_f64::<LittleEndian>()?;
        let lat = file.read_f64::<LittleEndian>()?;
        points.push(Point::new(lon, lat));
    }

    // Read highway tag (Phase 0)
    let highway_tag = if version >= FORMAT_VERSION_1 {
        let tag_present = file.read_u8()?;
        if tag_present == 1 {
            let tag_len = file.read_u32::<LittleEndian>()? as usize;
            let mut tag_bytes = vec![0u8; tag_len];
            file.read_exact(&mut tag_bytes)?;
            Some(String::from_utf8(tag_bytes).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e))
            })?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(MapObject {
        bounding_box,
        points,
        highway_tag,
    })
}

/// Read old format map object (version 0 - no tags)
fn read_map_object_v0(file: &mut File) -> io::Result<MapObject> {
    // Read bounding box (32 bytes)
    let min_lon = file.read_f64::<LittleEndian>()?;
    let min_lat = file.read_f64::<LittleEndian>()?;
    let max_lon = file.read_f64::<LittleEndian>()?;
    let max_lat = file.read_f64::<LittleEndian>()?;

    let bounding_box = BoundingBox {
        min: Point::new(min_lon, min_lat),
        max: Point::new(max_lon, max_lat),
    };

    // Read length (8 bytes)
    let points_len = file.read_i64::<LittleEndian>()?;

    // Read points
    let mut points = Vec::with_capacity(points_len as usize);
    for _ in 0..points_len {
        let lon = file.read_f64::<LittleEndian>()?;
        let lat = file.read_f64::<LittleEndian>()?;
        points.push(Point::new(lon, lat));
    }

    Ok(MapObject {
        bounding_box,
        points,
        highway_tag: None,
    })
}

/// Calculate the size of a map object in bytes
pub fn map_object_size(num_points: usize) -> usize {
    BOUNDING_BOX_SIZE + POINTS_LEN_SIZE + (num_points * POINT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::NamedTempFile;

    #[test]
    fn test_write_and_read_map_object_no_tag() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        let original = MapObject {
            bounding_box: BoundingBox {
                min: Point::new(10.0, 20.0),
                max: Point::new(30.0, 40.0),
            },
            points: vec![
                Point::new(15.0, 25.0),
                Point::new(20.0, 30.0),
                Point::new(25.0, 35.0),
            ],
            highway_tag: None,
        };

        // Write
        let offset = write_map_object(temp_file.as_file_mut(), &original)?;
        assert_eq!(offset, 0);

        // Read
        let read_obj = read_map_object(temp_file.as_file_mut(), offset)?;

        // Verify
        assert_eq!(
            read_obj.bounding_box.min.lon,
            original.bounding_box.min.lon
        );
        assert_eq!(
            read_obj.bounding_box.min.lat,
            original.bounding_box.min.lat
        );
        assert_eq!(
            read_obj.bounding_box.max.lon,
            original.bounding_box.max.lon
        );
        assert_eq!(
            read_obj.bounding_box.max.lat,
            original.bounding_box.max.lat
        );
        assert_eq!(read_obj.points.len(), original.points.len());
        for (i, point) in read_obj.points.iter().enumerate() {
            assert_eq!(point.lon, original.points[i].lon);
            assert_eq!(point.lat, original.points[i].lat);
        }
        assert_eq!(read_obj.highway_tag, None);

        Ok(())
    }

    #[test]
    fn test_write_and_read_map_object_with_tag() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        let original = MapObject::with_highway_tag(
            BoundingBox {
                min: Point::new(10.0, 20.0),
                max: Point::new(30.0, 40.0),
            },
            vec![
                Point::new(15.0, 25.0),
                Point::new(20.0, 30.0),
            ],
            Some("primary".to_string()),
        );

        // Write
        let offset = write_map_object(temp_file.as_file_mut(), &original)?;

        // Read
        let read_obj = read_map_object(temp_file.as_file_mut(), offset)?;

        // Verify geometry
        assert_eq!(read_obj.bounding_box.min.lon, 10.0);
        assert_eq!(read_obj.points.len(), 2);

        // Verify highway tag
        assert_eq!(read_obj.highway_tag, Some("primary".to_string()));

        Ok(())
    }

    #[test]
    fn test_binary_format_with_version() -> io::Result<()> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);

        let obj = MapObject::with_highway_tag(
            BoundingBox {
                min: Point::new(1.0, 2.0),
                max: Point::new(3.0, 4.0),
            },
            vec![Point::new(5.0, 6.0)],
            Some("motorway".to_string()),
        );

        write_map_object(&mut cursor, &obj)?;

        // Check format:
        // 8 (version) + 32 (bbox) + 8 (len) + 16 (point) + 1 (tag_present) + 4 (tag_len) + 8 (motorway)
        assert_eq!(buffer.len(), 77);

        // Verify version (first 8 bytes as u64)
        let mut cursor = Cursor::new(&buffer);
        let version = cursor.read_u64::<LittleEndian>()?;
        assert_eq!(version, FORMAT_VERSION_1);

        Ok(())
    }

    #[test]
    fn test_map_object_size() {
        // Note: This function doesn't account for version/tags yet
        // It's for the old calculation
        assert_eq!(map_object_size(0), 40); // 32 + 8 + 0
        assert_eq!(map_object_size(1), 56); // 32 + 8 + 16
        assert_eq!(map_object_size(10), 200); // 32 + 8 + 160
    }
}
