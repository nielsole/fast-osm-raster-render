#version 450

layout(location = 0) in vec2 position; // lon, lat

layout(set = 0, binding = 0) uniform UniformBufferObject {
    vec4 bbox;        // minLon, minLat, maxLon, maxLat
    float tileSize;   // 256.0
    mat4 projection;  // Orthographic projection
} ubo;

void main() {
    // Simple linear transformation for debugging
    // Maps lon/lat directly to NDC space without Mercator projection
    float x = position.x / 90.0;  // -180..180 -> -2..2 (clipped to -1..1)
    float y = position.y / 90.0;  // -90..90 -> -1..1
    gl_Position = vec4(x, y, 0.0, 1.0);
}
