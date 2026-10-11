#version 330 core

out vec4 FragColor;
in vec2 TexCoords;

uniform float u_time;
uniform float u_bass;
uniform float u_mids;
uniform float u_treble;
uniform float u_onset;
uniform float u_aspect;
uniform vec3 u_primary_color;
uniform vec3 u_secondary_color;

float soft_circle(float distance, float radius) {
    float edge = max(fwidth(distance), 0.0015);
    return 1.0 - smoothstep(radius - edge, radius + edge, distance);
}

void main() {
    vec2 position = TexCoords - 0.5;
    position.x *= u_aspect;

    float radius = length(position);
    float angle = atan(position.y, position.x);
    float bass = clamp(u_bass, 0.0, 1.0);
    float mids = clamp(u_mids, 0.0, 1.0);
    float treble = clamp(u_treble, 0.0, 1.0);

    float rotation = u_time;
    float petal_step = mids * 4.0;
    float petal_count = 5.0 + floor(petal_step);
    float petal_transition = smoothstep(0.72, 0.98, fract(petal_step));
    float lower_petals = cos(angle * petal_count + rotation);
    float upper_petals = cos(angle * (petal_count + 1.0) + rotation);
    float petal_wave = mix(lower_petals, upper_petals, petal_transition);
    float petal_phase = angle * petal_count + rotation;
    float gentle_sway = sin(angle * 3.0 - u_time);

    float bloom_radius = 0.19 + bass * 0.045 + u_onset * 0.075;
    float petal_depth = 0.025 + mids * 0.018;
    float flower_edge = bloom_radius
        + petal_depth * petal_wave
        + 0.009 * gentle_sway;
    float flower = soft_circle(radius - flower_edge, 0.0);

    float inner_radius = 0.078 + bass * 0.025;
    float inner_core = soft_circle(radius, inner_radius);
    float core_rim = 1.0 - smoothstep(
        0.006,
        0.014,
        abs(radius - inner_radius)
    );

    float outer_radius = 0.36 + bass * 0.025 + u_onset * 0.035;
    float ring_wave = sin(angle * 18.0 + u_time);
    float ring_radius = outer_radius + ring_wave * treble * 0.008;
    float ring_width = 0.002 + treble * 0.002;
    float outer_ring = 1.0 - smoothstep(
        ring_width,
        ring_width + max(fwidth(radius), 0.0015),
        abs(radius - ring_radius)
    );

    float petal_highlight = smoothstep(0.68, 0.96, petal_wave);
    float audio_shimmer = 0.82 + 0.18 * sin(petal_phase * 2.0 - u_time);
    float glint_phase = fract(angle * 12.0 / 6.2831853 + u_time / 6.2831853);
    float glint_distance = min(glint_phase, 1.0 - glint_phase);
    float glints = (1.0 - smoothstep(0.012, 0.035, glint_distance))
        * smoothstep(0.22, 0.38, radius)
        * (1.0 - smoothstep(0.43, 0.55, radius))
        * treble;

    vec3 ink = vec3(0.009, 0.012, 0.022);
    vec3 background = ink * (0.82 + 0.18 * exp(-radius * 2.0));
    float halo_radius = 0.29 + mids * 0.035 + u_onset * 0.055;
    float halo = 1.0 - smoothstep(
        0.001,
        0.008,
        abs(radius - halo_radius)
    );
    vec3 flower_color = mix(
        u_primary_color,
        u_secondary_color,
        petal_highlight * (0.18 + 0.28 * treble)
    );
    vec3 color = mix(background, flower_color * audio_shimmer, flower * 0.88);

    color = mix(color, u_secondary_color, core_rim * (0.2 + bass * 0.24 + u_onset * 0.3));
    color = mix(color, u_secondary_color, outer_ring * (0.1 + treble * 0.28));
    color = mix(color, u_primary_color, halo * (0.025 + u_onset * 0.08));
    color += u_secondary_color * glints * 0.12;
    color += u_primary_color * inner_core * (0.06 + bass * 0.1 + u_onset * 0.18);

    float vignette = 1.0 - smoothstep(0.48, 1.08, radius);
    color *= mix(0.4, 1.0, vignette);

    FragColor = vec4(color, 1.0);
}
