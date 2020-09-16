// Copyright 2016 Joe Wilm, The Alacritty Project Contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
#version 300 es
layout (location = 0) in vec2 aPos;
layout (location = 1) in vec2 aUv;
layout (location = 2) in vec3 aFg;
layout (location = 3) in float aFlags;

smooth out vec2 uv;
flat out vec3 fg;
flat out float flags;

uniform vec2 uScale;

void main()
{
    uv = aUv;
    fg = aFg;
		flags = aFlags;
		vec2 pos = vec2(-1., 1.) + aPos.xy * uScale;
    gl_Position = vec4(pos, 0.0, 1.0);
}
