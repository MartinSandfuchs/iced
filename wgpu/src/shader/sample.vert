#version 450

out gl_PerVertex {
    vec4 gl_Position;
};

layout(location = 0) out vec2 uv;

layout (set = 0, binding = 0) uniform Locals {
    mat4 u_Bounds;
};

layout (set = 0, binding = 1) uniform Globals {
    mat4 u_Transform;
};

// vec2 positions[6] = vec2[](vec2(-1.0, 1.0), vec2(1.0, 1.0), vec2(1.0, -1.0),
//                            vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(-1.0, -1.0));
vec2 positions[6] = vec2[](vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
                           vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 0.0));
// vec2 uvs[6] = vec2[](vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
//                      vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 0.0));


void main() {
    vec2 position = positions[gl_VertexIndex];
    uv = position;
    // uv = uvs[gl_VertexIndex];
    uv = vec2(uv.x, uv.y);
    gl_Position = u_Transform * u_Bounds * vec4(position, 0.0, 1.0);
}
