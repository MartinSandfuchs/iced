#version 450

layout(location = 0) in vec2 uv;

layout(set = 0, binding = 2) uniform sampler s_frame;
layout(set = 1, binding = 0) uniform texture2D t_frame;

layout(location = 0) out vec4 outColor;

void main() {
    vec3 color = texture(sampler2D(t_frame, s_frame), uv).rgb;
    outColor = vec4(color, 1.0);
}
