#version 450

// Per-object styling: Fragment shader with vertex color
// Color is passed from vertex shader (evaluated per-object)

layout(location = 0) in vec4 fragColor;  // From vertex shader
layout(location = 0) out vec4 outColor;

void main() {
    // Use the color from vertex data
    outColor = fragColor;
}
