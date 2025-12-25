#version 450

layout(location = 0) in vec2 position; // lon, lat

layout(set = 0, binding = 0) uniform UniformBufferObject {
    vec4 bbox;        // minLon, minLat, maxLon, maxLat
    float tileSize;   // 256.0
    mat4 projection;  // Orthographic projection
} ubo;

const float PI = 3.14159265359;
const float MAX_LAT = 85.0511287798; // Maximum latitude in Web Mercator

float lat2y_mercator(float lat) {
    // Clamp latitude to valid Mercator range
    lat = clamp(lat, -MAX_LAT, MAX_LAT);
    float latRad = lat * PI / 180.0;
    return log(tan(PI/4.0 + latRad/2.0));
}

void main() {
    // Convert longitude to x coordinate (linear)
    float x = (position.x - ubo.bbox.x) / (ubo.bbox.z - ubo.bbox.x);
    x = clamp(x, 0.0, 1.0);

    // Convert latitude to y coordinate (Mercator)
    float y_mercator = lat2y_mercator(position.y);
    float min_y_mercator = lat2y_mercator(ubo.bbox.y);
    float max_y_mercator = lat2y_mercator(ubo.bbox.w);
    float y = (y_mercator - min_y_mercator) / (max_y_mercator - min_y_mercator);
    y = clamp(y, 0.0, 1.0);

    // Map 0-1 normalized coordinates directly to NDC -1 to 1
    float ndc_x = x * 2.0 - 1.0;
    float ndc_y = (1.0 - y) * 2.0 - 1.0;  // Flip Y for correct orientation

    gl_Position = vec4(ndc_x, ndc_y, 0.0, 1.0);
}
