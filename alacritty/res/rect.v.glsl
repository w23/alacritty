#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec4 a_color;

uniform vec2 u_half_res;

flat out vec4 color;

void main() {
    color = a_color;
    gl_Position = vec4(a_pos / u_half_res - 1., 0.0, 1.0);
}
