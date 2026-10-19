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
uniform int u_vj_loops;
uniform int u_multiband_vj;
uniform float u_vj_seed;
uniform int u_audio_reactive;

float soft_circle(float distance, float radius) {
    float edge = max(fwidth(distance), 0.0015);
    return 1.0 - smoothstep(radius - edge, radius + edge, distance);
}

float vj_hash(float value) {
    return fract(sin(value * 127.1 + 311.7) * 43758.5453);
}

vec3 draw_multiband_vj(vec2 position, float bass, float mids, float treble, float onset) {
    float seed = u_vj_seed * 1000.0;
    float radius = length(position);
    float angle = atan(position.y, position.x);

    float bass_pulse = 0.5 + 0.5 * sin(u_time * 3.0 + seed);
    float bass_wave = abs(sin(radius * 38.0 - bass_pulse * 2.8));
    float bass_rings = smoothstep(0.88, 0.995, bass_wave)
        * smoothstep(0.08, 0.18, radius)
        * (1.0 - smoothstep(0.52, 0.72, radius));
    float bass_impact_radius = 0.12 + bass_pulse * 0.56;
    float bass_impact = 1.0 - smoothstep(
        0.006 + bass * 0.012,
        0.018 + bass * 0.025,
        abs(radius - bass_impact_radius)
    );

    float mid_warp = sin(radius * 8.0 - u_time * 2.0 + seed) * mids * 0.42;
    float mid_angle = angle + mid_warp;
    float mid_sectors = 6.0;
    float sector_width = 6.2831853 / mid_sectors;
    float folded_angle = mod(mid_angle + u_time + sector_width * 0.5, sector_width)
        - sector_width * 0.5;
    float mid_edge = 0.31 + 0.075 * cos(mid_angle * mid_sectors + u_time * 2.0 + seed);
    float mid_polygon_radius = mid_edge * cos(sector_width * 0.5) / max(cos(folded_angle), 0.25);
    float mid_shape = 1.0 - smoothstep(0.004, 0.014, abs(radius - mid_polygon_radius));
    float mid_orbits = smoothstep(0.92, 0.995, abs(sin(
        radius * 27.0 - mid_angle * 4.0 + u_time * 4.0
    ))) * smoothstep(0.12, 0.2, radius) * (1.0 - smoothstep(0.5, 0.68, radius));

    float spoke_phase = angle * 28.0 + u_time * 5.0 + seed;
    float high_spokes = pow(max(cos(spoke_phase), 0.0), 36.0)
        * smoothstep(0.08, 0.2, radius)
        * (1.0 - smoothstep(0.48, 0.68, radius));
    vec2 sparkle_cell = floor(position * 76.0);
    vec2 sparkle_local = fract(position * 76.0) - 0.5;
    float sparkle_hash = vj_hash(dot(sparkle_cell, vec2(1.0, 57.0)) + seed);
    float sparkle_pulse = 0.5 + 0.5 * sin(u_time * 11.0 + sparkle_hash * 6.2831853);
    float high_sparkles = (1.0 - smoothstep(0.025, 0.13, length(sparkle_local)))
        * step(0.82, sparkle_hash)
        * smoothstep(0.01, 0.12, treble)
        * sparkle_pulse;

    vec3 ink = vec3(0.004, 0.006, 0.016);
    vec3 high_color = min(vec3(1.0), mix(u_primary_color, u_secondary_color, 0.5) + 0.35);
    vec3 color = ink;
    color += u_primary_color * bass_rings * (0.18 + bass * 0.92);
    color += u_primary_color * bass_impact * (0.08 + bass * 0.8 + onset * 0.35);
    color += u_secondary_color * mid_shape * (0.12 + mids * 0.88);
    color += u_secondary_color * mid_orbits * (0.08 + mids * 0.48);
    color += high_color * high_spokes * smoothstep(0.015, 0.12, treble) * 0.9;
    color += high_color * high_sparkles * (0.25 + treble * 1.3);
    float vignette = 1.0 - smoothstep(0.58, 1.0, radius);
    return color * mix(0.3, 1.0, vignette);
}

vec3 draw_vj_loops(
    vec2 position,
    float bass,
    float mids,
    float treble,
    float onset
) {
    float seed = u_vj_seed * 1000.0;
    float style = floor(vj_hash(seed + 2.0) * 4.0);
    float sectors = 5.0 + floor(vj_hash(seed + 3.0) * 8.0);
    float layers = 4.0 + floor(vj_hash(seed + 4.0) * 4.0);
    float spin = vj_hash(seed + 5.0) < 0.5 ? -1.0 : 1.0;
    float wave_frequency = 1.0 + floor(vj_hash(seed + 13.0) * 4.0);
    float twist = mix(0.35, 2.1, vj_hash(seed + 6.0));
    float scale = mix(0.78, 1.18, vj_hash(seed + 7.0));
    float offset_angle = vj_hash(seed + 8.0) * 6.2831853;
    vec2 center_offset = (vec2(vj_hash(seed + 9.0), vj_hash(seed + 10.0)) - 0.5) * 0.14;
    vec2 p = position - center_offset;
    float radius = length(p);
    float angle = atan(p.y, p.x) + offset_angle;
    float rotation = u_time * spin;
    float mid_warp = sin(radius * 9.0 - u_time * wave_frequency + offset_angle) * mids * 0.24;
    angle += mid_warp;
    radius += sin(angle * sectors + u_time * 2.0) * mids * 0.045;
    float sector_angle = 6.2831853 / sectors;
    float folded_angle = mod(angle + rotation + sector_angle * 0.5, sector_angle)
        - sector_angle * 0.5;
    float polygon_edge = 0.34 * scale * cos(sector_angle * 0.5)
        / max(cos(folded_angle), 0.2);
    float pulse = bass * 0.09 + onset * 0.15;
    float rings = 0.0;
    float orbit_lines = 0.0;
    float petals = cos(angle * sectors + rotation + sin(radius * 9.0 - rotation) * twist);
    float radial_wave = sin(radius * (30.0 + vj_hash(seed + 11.0) * 28.0)
        - rotation * wave_frequency + sin(angle * sectors) * 2.0);

    for (int layer = 0; layer < 7; layer++) {
        float index = float(layer);
        if (index < layers) {
            float base_radius = (0.11 + index * (0.43 / layers)) * scale;
            float layer_rotation = rotation * (1.0 + index) + index * twist;
            float layer_sectors = max(3.0, sectors - mod(index, 3.0) * 2.0);
            float layer_step = 6.2831853 / layer_sectors;
            float layer_angle = mod(angle + layer_rotation + layer_step * 0.5, layer_step)
                - layer_step * 0.5;
            float layer_edge = base_radius * cos(layer_step * 0.5)
                / max(cos(layer_angle), 0.2);
            float layer_radius = base_radius + petals * (0.012 + mids * 0.018)
                + bass * 0.035 + onset * 0.055;
            float ring_distance = abs(radius - layer_radius);
            if (style < 1.0) {
                ring_distance = abs(radius - layer_edge);
            } else if (style < 2.0) {
                ring_distance = abs(radius - base_radius
                    - petals * (0.018 + mids * 0.025));
            } else if (style < 3.0) {
                float orbit_angle = angle + layer_rotation;
                float ellipse = length(vec2(
                    p.x * (0.8 + mod(index, 3.0) * 0.2),
                    p.y * (1.2 - mod(index, 3.0) * 0.1)
                ));
                ring_distance = abs(ellipse - base_radius
                    - 0.035 * sin(orbit_angle * 2.0 + rotation));
            } else {
                ring_distance = abs(sin(radius * (27.0 + index * 4.0)
                    - angle * sectors * 0.32 + rotation * wave_frequency)) * 0.018;
            }

            float line_width = 0.0025 + treble * 0.0015;
            float line = 1.0 - smoothstep(line_width, line_width + 0.007, ring_distance);
            rings += line * (1.0 - index * 0.075);
            orbit_lines += (1.0 - smoothstep(0.003, 0.012, abs(
                radius - base_radius - 0.025 * sin(angle * layer_sectors + layer_rotation)
            ))) * step(2.0, style) * (1.0 - step(3.0, style));
        }
    }

    float beams = pow(max(0.0, cos(angle * sectors + rotation)), 18.0)
        * smoothstep(0.17, 0.38, radius)
        * (1.0 - smoothstep(0.54, 0.66, radius));
    float lattice = smoothstep(0.82, 0.99, abs(radial_wave))
        * smoothstep(0.12, 0.2, radius)
        * (1.0 - smoothstep(0.48, 0.6, radius));
    float polygon = 1.0 - smoothstep(0.003, 0.012, abs(radius - polygon_edge * (1.0 + pulse)));
    float center_flash = exp(-radius * (9.0 + vj_hash(seed + 12.0) * 8.0))
        * (0.12 + bass * 0.32 + onset * 0.4);
    float impact_radius = mix(0.08, 0.72, 1.0 - onset);
    float impact_ring = (1.0 - smoothstep(
        0.006 + bass * 0.012,
        0.022 + bass * 0.02,
        abs(radius - impact_radius)
    )) * onset;
    vec2 sparkle_cell = floor(position * 68.0);
    vec2 sparkle_local = fract(position * 68.0) - 0.5;
    float sparkle_hash = vj_hash(dot(sparkle_cell, vec2(1.0, 57.0)) + seed);
    float sparkle_pulse = 0.5 + 0.5 * sin(u_time * (2.0 + floor(sparkle_hash * 4.0))
        + sparkle_hash * 6.2831853);
    float sparkle = (1.0 - smoothstep(0.015, 0.12, length(sparkle_local)))
        * step(1.0 - treble * 0.82, sparkle_hash)
        * smoothstep(0.025, 0.16, treble)
        * sparkle_pulse;
    float shimmer = 0.65 + 0.35 * sin(angle * (sectors * 0.5) - rotation + radius * 11.0);

    vec3 ink = vec3(0.004, 0.006, 0.016);
    vec3 gradient = mix(
        u_primary_color,
        u_secondary_color,
        0.5 + 0.5 * sin(angle * (2.0 + mod(sectors, 5.0)) + rotation + radius * 12.0)
    );
    vec3 color = ink * (0.78 + 0.22 * exp(-radius * 2.0));
    color += gradient * rings * (0.22 + treble * 0.24);
    color += u_secondary_color * orbit_lines * (0.24 + mids * 0.25);
    color += u_primary_color * beams * (0.16 + mids * 0.18);
    color += gradient * lattice * (0.12 + treble * 0.24);
    color = mix(color, u_secondary_color, polygon * 0.5);
    color += u_secondary_color * impact_ring * (0.45 + bass * 0.55);
    color += u_primary_color * sparkle * (0.45 + treble * 1.2);
    color += u_primary_color * center_flash;
    color *= 0.78 + shimmer * 0.22;

    float vignette = 1.0 - smoothstep(0.55, 1.12, radius);
    return color * mix(0.32, 1.0, vignette);
}

void main() {
    vec2 position = TexCoords - 0.5;
    position.x *= u_aspect;

    float radius = length(position);
    float angle = atan(position.y, position.x);
    float bass = clamp(u_bass, 0.0, 1.0);
    float mids = clamp(u_mids, 0.0, 1.0);
    float treble = clamp(u_treble, 0.0, 1.0);
    float onset = clamp(u_onset, 0.0, 1.0);

    if (u_vj_loops != 0 || u_multiband_vj != 0) {
        if (u_audio_reactive == 0) {
            bass = 0.28 + 0.22 * (0.5 + 0.5 * cos(u_time * 2.0));
            mids = 0.22 + 0.2 * (0.5 + 0.5 * sin(u_time * 3.0 + u_vj_seed * 6.2831853));
            treble = 0.18 + 0.2 * (0.5 + 0.5 * sin(u_time * 5.0));
            onset = pow(max(0.0, cos(u_time * 4.0 + u_vj_seed * 6.2831853)), 18.0);
        }
        if (u_multiband_vj != 0) {
            FragColor = vec4(draw_multiband_vj(position, bass, mids, treble, onset), 1.0);
            return;
        }
        FragColor = vec4(draw_vj_loops(position, bass, mids, treble, onset), 1.0);
        return;
    }

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
