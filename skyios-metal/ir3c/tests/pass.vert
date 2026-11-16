#version 450
// VS that passes through position (like sky_position_vertex: VFD writes
// float4 position to r0, VS just does 'end')
layout(location = 0) in vec4 position;
void main() { gl_Position = position; }
