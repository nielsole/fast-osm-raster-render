#version 450

layout(location = 0) in vec2 position; // lon, lat

layout(set = 0, binding = 0) uniform UniformBufferObject {
    vec4 bbox;
    float tileSize;
    mat4 projection;
} ubo;

void main() {
    // DEBUG: Draw an X pattern across the screen
    // Alternate between corners to create visible lines
    int idx = gl_VertexIndex % 4;
    if (idx == 0) {
        gl_Position = vec4(-0.5, -0.5, 0.0, 1.0); // Bottom-left
    } else if (idx == 1) {
        gl_Position = vec4(0.5, 0.5, 0.0, 1.0);   // Top-right
    } else if (idx == 2) {
        gl_Position = vec4(-0.5, 0.5, 0.0, 1.0);  // Top-left
    } else {
        gl_Position = vec4(0.5, -0.5, 0.0, 1.0);  // Bottom-right
    }
}
