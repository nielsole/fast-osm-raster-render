use super::types::{BoundingBox, MapObject, MapObjectOffset, Point};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::File;
use std::io::{self, Seek, SeekFrom};

/// Binary format (must match Go version):
/// - BoundingBox: 32 bytes
///   - min.lon: 8 bytes (f64)
///   - min.lat: 8 bytes (f64)
///   - max.lon: 8 bytes (f64)
///   - max.lat: 8 bytes (f64)
/// - points_len: 8 bytes (i64)
/// - points: points_len * 16 bytes
///   - each point: lon (8 bytes f64) + lat (8 bytes f64)

pub const BOUNDING_BOX_SIZE: usize = 32;
pub const POINTS_LEN_SIZE: usize = 8;
pub const POINT_SIZE: usize = 16;

/// Write a map object to a writer and return its offset
pub fn write_map_object<W: WriteBytesExt + Seek>(writer: &mut W, obj: &MapObject) -> io::Result<MapObjectOffset> {
    let offset = writer.stream_position()?;

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

    Ok(offset)
}

/// Read a map object from a file at a given offset
pub fn read_map_object(file: &mut File, offset: MapObjectOffset) -> io::Result<MapObject> {
    file.seek(SeekFrom::Start(offset))?;

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
    fn test_write_and_read_map_object() -> io::Result<()> {
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

        Ok(())
    }

    #[test]
    fn test_map_object_size() {
        assert_eq!(map_object_size(0), 40); // 32 + 8 + 0
        assert_eq!(map_object_size(1), 56); // 32 + 8 + 16
        assert_eq!(map_object_size(10), 200); // 32 + 8 + 160
    }

    #[test]
    fn test_binary_layout() -> io::Result<()> {
        // Test that the binary layout matches Go's expectations
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);

        let obj = MapObject {
            bounding_box: BoundingBox {
                min: Point::new(1.0, 2.0),
                max: Point::new(3.0, 4.0),
            },
            points: vec![Point::new(5.0, 6.0)],
        };

        write_map_object(&mut cursor, &obj)?;

        // Check total size
        assert_eq!(buffer.len(), 56); // 32 + 8 + 16

        // Check that we can read back the bounding box
        let mut cursor = Cursor::new(&buffer);
        let min_lon = cursor.read_f64::<LittleEndian>()?;
        let min_lat = cursor.read_f64::<LittleEndian>()?;
        let max_lon = cursor.read_f64::<LittleEndian>()?;
        let max_lat = cursor.read_f64::<LittleEndian>()?;
        let points_len = cursor.read_i64::<LittleEndian>()?;
        let point_lon = cursor.read_f64::<LittleEndian>()?;
        let point_lat = cursor.read_f64::<LittleEndian>()?;

        assert_eq!(min_lon, 1.0);
        assert_eq!(min_lat, 2.0);
        assert_eq!(max_lon, 3.0);
        assert_eq!(max_lat, 4.0);
        assert_eq!(points_len, 1);
        assert_eq!(point_lon, 5.0);
        assert_eq!(point_lat, 6.0);

        Ok(())
    }
}
