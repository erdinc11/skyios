#version 450
// framebuffer fetch via Vulkan subpass input attachment (maps to ir3 fb_read
// path on a6xx: sampling the color attachment, same as CoreAnimation's
// framebuffer_fetch=1 blending pattern).
// out = 0.5 * framebuffer + vec4(0.5, 0, 0, 0.5) — same as accel-test 7
layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput fbIn;
layout(location = 0) out vec4 outColor;
void main() {
    vec4 fb = subpassLoad(fbIn);
    outColor = 0.5 * fb + vec4(0.5, 0.0, 0.0, 0.5);
}
