#version 450

// Per-object styling: vertex shader with color passthrough

layout(location = 0) in vec2 inPosition;  // (lon, lat)
layout(location = 1) in vec4 inColor;     // (r, g, b, a)

layout(location = 0) out vec4 fragColor;  // Pass to fragment shader

layout(set = 0, binding = 0) uniform UniformBufferObject {
    vec4 bbox;        // minLon, minLat, maxLon, maxLat
    float tileSize;   // 256.0 or 512.0
    float padding[11];
    mat4 projection;  // 4x4 matrix (unused in Mercator shader)
} ubo;

// Web Mercator projection
float mercatorY(float lat) {
    float latRad = radians(lat);
    return log(tan(latRad) + 1.0 / cos(latRad));
}

void main() {
    float lon = inPosition.x;
    float lat = inPosition.y;

    // Normalize to [0, 1] within bbox
    float x = (lon - ubo.bbox.x) / (ubo.bbox.z - ubo.bbox.x);

    // Mercator projection for Y
    float mercY = mercatorY(lat);
    float mercMinY = mercatorY(ubo.bbox.y);
    float mercMaxY = mercatorY(ubo.bbox.w);
    float y = (mercY - mercMinY) / (mercMaxY - mercMinY);

    // Flip Y for screen coordinates (0 at top)
    y = 1.0 - y;

    // Convert to NDC [-1, 1]
    vec2 ndc = vec2(x, y) * 2.0 - 1.0;

    gl_Position = vec4(ndc, 0.0, 1.0);

    // Pass color to fragment shader
    fragColor = inColor;
}
