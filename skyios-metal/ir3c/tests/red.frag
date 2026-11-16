#version 450
// smoke test: constant colour, the same thing the driver's hand-assembled
// sky_red_fragment does (mov.f32f32 r0 = 1,0,0,1 ; end)
layout(location = 0) out vec4 outColor;
void main() { outColor = vec4(1.0, 0.0, 0.0, 1.0); }
