shader_type canvas_item;
// The dot display's composite (DotDisplay): turns the display layers into an amber LED
// dot matrix on a grid fixed to the world. Each grid cell becomes one dot, sized by its
// brightness and coloured by what's in it, by priority:
// - information drawn as solid dots (transit lines, signals, overlays, tool previews):
//   a round dot in its own colour, over everything;
// - light: strong light keeps its colour (white headlamps, red brake lights, orange
//   indicators);
// - actors (vehicles, people): lit by the ambient light only, plus their own lamps, so
//   the beam of the car behind never shows on them; buses keep their line's colour;
// - information drawn as marks on the ground, under the actors: square dots (bus lanes,
//   stop signs; as big and as coloured as the mark is opaque) and hollow rings on every
//   other cell (zones);
// - the map (ground, roads, buildings): dimmed, lit by ambient light and by the light
//   layer (lamps, windows, beams).
// The layers are drawn unlit and premultiplied (transparent viewports); light adds up.

uniform sampler2D map_layer: filter_linear, repeat_disable;
uniform sampler2D actor_layer: filter_linear, repeat_disable;
uniform sampler2D light_layer: filter_linear, repeat_disable;
uniform sampler2D info_layer: filter_linear, repeat_disable;
uniform sampler2D square_layer: filter_linear, repeat_disable;
uniform sampler2D ring_layer: filter_linear, repeat_disable;
// World position of the screen's top-left pixel, and world px per screen px.
uniform vec2 world_origin = vec2(0.0);
uniform float world_per_px = 1.0;
// The grid's cell, in world px.
uniform float cell = 4.0;
// 0 by day, 1 at night.
uniform float night = 0.0;

const float LEVELS = 16.0;
const float AMBIENT_NIGHT = 0.4;
// Actors stay more visible at night than the map: what moves matters most.
const float ACTOR_AMBIENT_NIGHT = 0.6;
const float MAP_GAIN = 0.75;
const float ACTOR_GAIN = 1.5;
// Above this, light on an actor is its own lamp (vehicle_light.gdshader), not a beam.
const float LAMP = 0.85;
// Information shows in a cell once any of its samples is at least this covered, so a
// line thinner than the cell still shows.
const float INFO_COVER = 0.5;
const float MARK_COVER = 0.1;
const vec3 AMBER_DIM = vec3(0.24, 0.11, 0.02);
const vec3 AMBER_BRIGHT = vec3(1.0, 0.85, 0.5);
const vec3 WHITE = vec3(1.0, 0.98, 0.94);
const vec3 BACKGROUND = vec3(0.027, 0.016, 0.008);
const vec3 UNLIT_DOT = vec3(0.1, 0.07, 0.035);
// Dot shapes.
const int ROUND = 0;
const int SQUARE = 1;
const int RING = 2;

float lum(vec3 c) {
	return dot(c, vec3(0.3, 0.59, 0.11));
}

float saturation(vec3 c) {
	float high = max(c.r, max(c.g, c.b));
	return high <= 0.0 ? 0.0 : (high - min(c.r, min(c.g, c.b))) / high;
}

// The same hue, saturated and at `level` brightness (white stays white).
vec3 vivid(vec3 c, float level) {
	float high = max(c.r, max(c.g, c.b));
	vec3 hue = c / max(high, 1e-4);
	float low = min(hue.r, min(hue.g, hue.b));
	if (low < 0.8) {
		hue = (hue - low * 0.85) / max(1.0 - low * 0.85, 1e-4);
	}
	return hue * (0.45 + 0.55 * level);
}

// An information layer's samples in a cell: the most covered sample's colour
// (un-premultiplied) and that coverage.
struct Mark {
	vec3 color;
	float cover;
};

void take(inout Mark mark, vec4 sample) {
	if (sample.a > mark.cover) {
		mark.cover = sample.a;
		mark.color = sample.rgb / max(sample.a, 1e-4);
	}
}

void fragment() {
	vec2 pixel = FRAGCOORD.xy;
	vec2 world = world_origin + pixel * world_per_px;
	vec2 cell_index = floor(world / cell);
	vec2 center = ((cell_index + 0.5) * cell - world_origin) / world_per_px;
	float pitch = cell / world_per_px;

	// Nine samples across the cell.
	float map_sum = 0.0;
	vec4 actor_sum = vec4(0.0);
	vec3 light_sum = vec3(0.0);
	vec3 light_peak = vec3(0.0);
	Mark info = Mark(vec3(0.0), 0.0);
	Mark square = Mark(vec3(0.0), 0.0);
	Mark ring = Mark(vec3(0.0), 0.0);
	for (int i = -1; i <= 1; i++) {
		for (int j = -1; j <= 1; j++) {
			vec2 uv = (center + vec2(float(i), float(j)) * pitch / 3.0) * SCREEN_PIXEL_SIZE;
			map_sum += lum(texture(map_layer, uv).rgb);
			actor_sum += texture(actor_layer, uv);
			take(info, texture(info_layer, uv));
			take(square, texture(square_layer, uv));
			take(ring, texture(ring_layer, uv));
			vec3 light = texture(light_layer, uv).rgb;
			light_sum += light;
			if (lum(light) > lum(light_peak)) {
				light_peak = light;
			}
		}
	}
	float ambient = mix(1.0, AMBIENT_NIGHT, night);
	float actor_ambient = mix(1.0, ACTOR_AMBIENT_NIGHT, night);
	// Information is lit too, but never darker than this: it must stay readable.
	float info_ambient = max(ambient, 0.75);
	float actor_cover = actor_sum.a / 9.0;
	vec3 actor = actor_sum.rgb / max(actor_sum.a, 1e-4);
	bool on_actor = actor_cover > 0.35;
	vec3 light = on_actor ? (lum(light_peak) > LAMP ? light_peak : vec3(0.0)) : light_sum / 9.0;

	float level;
	vec3 color;
	int shape = ROUND;
	float strength = 1.0;
	if (info.cover > INFO_COVER) {
		level = 0.85 * info_ambient;
		color = vivid(info.color, level);
	} else if (!on_actor && lum(light) > 0.5 && square.cover <= MARK_COVER) {
		// Light on the ground, brighter than any mark under it.
		level = clamp(map_sum / 9.0 * MAP_GAIN * ambient + lum(light), 0.0, 1.0);
		color = saturation(light) > 0.5 ? vivid(light, level) : (lum(light_peak) > LAMP ? WHITE : mix(AMBER_DIM, AMBER_BRIGHT, ceil(level * LEVELS) / LEVELS));
	} else if (!on_actor && square.cover > MARK_COVER) {
		// As strong as the mark is opaque: a bus lane's faint tint gives small, muted
		// squares leaning to amber; a stop sign full, bright ones.
		strength = square.cover;
		level = clamp((0.2 + 0.65 * strength) * info_ambient + lum(light) * 0.5, 0.0, 1.0);
		color = mix(mix(AMBER_DIM, AMBER_BRIGHT, level), vivid(square.color, level), strength);
		shape = SQUARE;
	} else if (!on_actor && ring.cover > MARK_COVER && mod(cell_index.x + cell_index.y, 2.0) < 0.5) {
		level = 0.7 * info_ambient;
		color = vivid(ring.color, level);
		shape = RING;
	} else {
		float surface = on_actor ? lum(actor) * ACTOR_GAIN * actor_ambient : map_sum / 9.0 * MAP_GAIN * ambient;
		level = clamp(surface + lum(light), 0.0, 1.0);
		if (lum(light) > surface && saturation(light) > 0.5) {
			color = vivid(light, level);
		} else if (lum(light_peak) > LAMP && lum(light) > surface && saturation(light_peak) < 0.3) {
			color = WHITE;
		} else if (on_actor && saturation(actor) > 0.55) {
			level = max(level, 0.6 * actor_ambient + 0.3);
			color = vivid(actor, level);
		} else {
			color = mix(AMBER_DIM, AMBER_BRIGHT, ceil(level * LEVELS) / LEVELS);
		}
	}

	// Even the brightest dots keep a gap between them, so the panel never turns solid.
	vec2 offset = pixel - center;
	float d = length(offset);
	float lit;
	if (shape == SQUARE) {
		float half_size = pitch * (0.2 + 0.16 * strength);
		float edge = max(abs(offset.x), abs(offset.y));
		lit = 1.0 - smoothstep(half_size - 0.5, half_size + 0.25, edge);
	} else if (shape == RING) {
		float outer = pitch * 0.42;
		float inner = max(outer - max(pitch * 0.16, 0.8), 0.0);
		lit = (1.0 - smoothstep(outer - 0.5, outer + 0.25, d)) * smoothstep(inner - 0.25, inner + 0.5, d);
	} else {
		float radius = level < 0.06 ? 0.0 : pitch * 0.42 * min(1.0, 0.3 + level * 0.8);
		lit = 1.0 - smoothstep(radius - 0.5, radius + 0.25, d);
	}
	float unlit = 1.0 - smoothstep(pitch * 0.2 - 0.5, pitch * 0.2 + 0.25, d);
	COLOR = vec4(mix(mix(BACKGROUND, UNLIT_DOT, unlit), color, lit), 1.0);
}
