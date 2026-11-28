enable wgpu_ray_query;

// The solver's bounce light around a point (nvf-rtgi gather.rs `probe`), over the words of tables.rs. Entry points:
// `solve` writes a point's moments and counts, `replay` what each single ray met and brought, both of them `gather`;
// `reach` a point's hits and backs alone (gather.rs `counted`), with the same rays and surfaces and nothing lit;
// `meetings` the solver's triangle test of point i against primitive i.
//
// Of surfaces in one place the rounding of the structure's distances picks one, and that of the solver's another.
// The triangles near a point (tables.rs `Points`, its packed lists) and all that is about the structure's nearest one
// are decided by the solver's own test, in its own numbers (`meet`), and their alpha by its own weights.
//
// A workgroup is a point: each of its invocations traces every 64th ray, then the first one sums them, always in the
// same order, and writes the point whole.
//
// No value is ever infinite or not a number: tables.rs and live.rs refuse what could make the products and sums here
// overflow, and the guards are at `power` (no pow of 0 or less), `display_linear` (the same), `shading` (a normal that
// vanishes), `lamp_light` (a lamp at the place itself) and the shadow segment's length.

const GROUP: u32 = 64u;
// Corners are within 1e5 (tables.rs).
const FAR: f32 = 1e7;
const NONE: u32 = 0xffffffffu;
const FACE: u32 = 21u;
const LAMP: u32 = 16u;
const PLACE: u32 = 7u;
// A direction's words (tables.rs): as given, for the moments; as the rays go, for the structure, the triangle test
// and the side met; the shear, the axes and the still ones, the reciprocals.
const DIRECTION: u32 = 13u;
const GIVEN: u32 = 0u;
const ALONG: u32 = 3u;
const SHEAR: u32 = 6u;
const AXES: u32 = 9u;
const INVERSE: u32 = 10u;
const NEAR: u32 = 7u;
const MOMENTS: u32 = 16u;
const ANSWER: u32 = 20u;
const MEETING: u32 = 8u;
// Of a face's material word.
const UV_ZERO: u32 = 0x80000000u;
// Materials' flags.
const BACK_CULLED: u32 = 1u;
const FRONT_CULLED: u32 = 2u;
const DOUBLE_SIDED: u32 = 4u;
const UNLIT: u32 = 8u;
const DISPLAY_COLORS: u32 = 16u;
// Off a surface (along its normal) that the solver's nearest one is no further from; of a ray along it the window is no
// wider than this.
const WINDOW: f32 = 0.02;
const GRAZING: f32 = 1e-3;
const WIDEST: f32 = 4.0;

// live.rs `slot`: words in this order.
struct Params {
    // Of the order: the batch's first point, and how many there are.
    first: u32,
    count: u32,
    rays: u32,
    // The primitives of the first geometry, when there are two.
    opaque: u32,
    textures_at: u32,
    colors_at: u32,
    linear_at: u32,
    directions_at: u32,
    order_at: u32,
    emission: f32,
    // The structure's queries start after this distance: above 0, so that a surface at 0 is not met (bvh.rs `closest`:
    // t > near). The solver's own test meets above 0, as on the CPU.
    near: f32,
    // 0, which no compiler knows (`keep`).
    zero: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    pad3: u32,
}

@group(0) @binding(0) var scene: acceleration_structure;
// What the bottom level structure is built from: three corners a primitive.
@group(0) @binding(1) var<storage, read> corners: array<f32>;
@group(0) @binding(2) var<storage, read> faces: array<u32>;
// Materials, textures, vertex colours, the sRGB table, directions.
@group(0) @binding(3) var<storage, read> data: array<u32>;
// RGBA8 colour levels, and alpha levels of a byte a texel.
@group(0) @binding(4) var<storage, read> texels: array<u32>;
@group(0) @binding(5) var<storage, read> lights: array<u32>;
// The points, then their order, then the triangles near them, with their boxes.
@group(0) @binding(6) var<storage, read> places: array<u32>;
@group(0) @binding(7) var<uniform> p: Params;
// Moments, answers, meetings or hits and backs, by the entry point.
@group(0) @binding(8) var<storage, read_write> out: array<u32>;

fn df(i: u32) -> f32 {
    return bitcast<f32>(data[i]);
}

fn df3(i: u32) -> vec3<f32> {
    return vec3<f32>(df(i), df(i + 1u), df(i + 2u));
}

fn ff(i: u32) -> f32 {
    return bitcast<f32>(faces[i]);
}

fn ff3(i: u32) -> vec3<f32> {
    return vec3<f32>(ff(i), ff(i + 1u), ff(i + 2u));
}

fn lf(i: u32) -> f32 {
    return bitcast<f32>(lights[i]);
}

fn lf3(i: u32) -> vec3<f32> {
    return vec3<f32>(lf(i), lf(i + 1u), lf(i + 2u));
}

fn corner(tri: u32, k: u32) -> vec3<f32> {
    let i = 9u * tri + 3u * k;
    return vec3<f32>(corners[i], corners[i + 1u], corners[i + 2u]);
}

// A part of direction `r`'s row.
fn direction(r: u32, part: u32) -> vec3<f32> {
    return df3(p.directions_at + DIRECTION * r + part);
}

// A primitive's number in the tables.
fn number(geometry: u32, primitive: u32) -> u32 {
    return select(0u, p.opaque, geometry == 1u) + primitive;
}

fn weights(b: vec2<f32>) -> vec3<f32> {
    return vec3<f32>(1.0 - b.x - b.y, b.x, b.y);
}

fn material(tri: u32) -> u32 {
    return (faces[tri * FACE] & 0x7fffffffu) * 16u;
}

// ---- The solver's arithmetic.
//
// Sums, differences and products of f32 are rounded to the nearest; what is not given is that a product and a sum stay
// two roundings, or that a rounding error is not simplified away. `keep` keeps them: no compiler knows `zero`.
fn keep(x: f32) -> f32 {
    return bitcast<f32>(bitcast<u32>(x) ^ p.zero);
}

fn times(a: f32, b: f32) -> f32 {
    return keep(a * b);
}

// a.x w.x + a.y w.y + a.z w.z, in that order: the solver's blends and dot products.
fn blend(a: vec3<f32>, w: vec3<f32>) -> f32 {
    return keep(keep(times(a.x, w.x) + times(a.y, w.y)) + times(a.z, w.z));
}

fn mix4(w: vec4<f32>, a: vec4<f32>) -> f32 {
    return keep(keep(keep(times(w.x, a.x) + times(w.y, a.y)) + times(w.z, a.z)) + times(w.w, a.w));
}

// A sum or a product, rounded, and what the rounding left out.
fn two_sum(a: f32, b: f32) -> vec2<f32> {
    let s = keep(a + b);
    let c = keep(s - a);
    return vec2<f32>(s, keep(keep(a - keep(s - c)) + keep(b - c)));
}

fn two_product(a: f32, b: f32) -> vec2<f32> {
    let x = times(a, b);
    let c = times(4097.0, a);
    let ah = keep(c - keep(c - a));
    let al = keep(a - ah);
    let d = times(4097.0, b);
    let bh = keep(d - keep(d - b));
    let bl = keep(b - bh);
    let e = keep(keep(keep(keep(times(ah, bh) - x) + times(ah, bl)) + times(al, bh)) + times(al, bl));
    return vec2<f32>(x, e);
}

// a.x b.y - a.y b.x as the solver has it from f64, rounded to the nearest f32. Of the three parts of the sum the two
// lesser ones are rounded to odd, so that the last rounding is the only one.
fn fine(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let x = two_product(a.x, b.y);
    let y = two_product(a.y, b.x);
    let s = two_sum(x.x, -y.x);
    let u = two_sum(x.y, -y.y);
    let v = two_sum(s.y, u.x);
    let w = two_sum(s.x, v.x);
    let l = two_sum(w.y, keep(v.y + u.y));
    var odd = l.x;
    if l.y != 0.0 && (bitcast<u32>(l.x) & 1u) == 0u && l.x != 0.0 {
        // Away from 0 or towards it, to where the rest is.
        odd = bitcast<f32>(select(bitcast<u32>(l.x) - 1u, bitcast<u32>(l.x) + 1u, (l.x > 0.0) == (l.y > 0.0)));
    }
    return keep(w.x + odd);
}

fn edge(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return keep(times(a.x, b.y) - times(a.y, b.x));
}

fn left(n: f32, q: f32, d: f32) -> f32 {
    let x = two_product(q, d);
    return keep(keep(n - x.x) - x.y);
}

// The quotient, rounded to the nearest: WGSL's is 2.5 steps off at most. Neither number is 0 or infinite.
fn quotient(n: f32, d: f32) -> f32 {
    var q = keep(n / d);
    q = keep(q + keep(left(n, q, d) / d));
    let r = left(n, q, d);
    let step_up = keep(bitcast<f32>(bitcast<u32>(q) + 1u) - q);
    if q != 0.0 && abs(r) > times(0.5 * abs(step_up), abs(d)) {
        let up = (r > 0.0) == (d > 0.0);
        q = bitcast<f32>(select(bitcast<u32>(q) - 1u, bitcast<u32>(q) + 1u, up == (q > 0.0)));
    }
    return q;
}

// n / d of a d that is not 0, of an n that may be.
fn share(n: f32, d: f32) -> f32 {
    if n == 0.0 {
        return 0.0;
    }
    return quotient(n, d);
}

// ---- Surfaces.

fn wrap(i: i32, n: i32, how: u32) -> i32 {
    if how == 0u {
        return ((i % n) + n) % n;
    }
    if how == 1u {
        let m = ((i % (2 * n)) + 2 * n) % (2 * n);
        return min(m, 2 * n - 1 - m);
    }
    return clamp(i, 0, n - 1);
}

fn texture_at(tri: u32, m: u32, w: vec3<f32>) -> vec2<f32> {
    let f = tri * FACE;
    if (faces[f] & UV_ZERO) != 0u {
        return vec2<f32>(0.0, 0.0);
    }
    let u = blend(vec3<f32>(ff(f + 14u), ff(f + 16u), ff(f + 18u)), w);
    let v = blend(vec3<f32>(ff(f + 15u), ff(f + 17u), ff(f + 19u)), w);
    // No texel's number is past an i32 (tables.rs).
    let s = keep(keep(times(df(m + 4u), u) + times(df(m + 5u), v)) + df(m + 6u));
    let t = keep(keep(times(df(m + 8u), u) + times(df(m + 9u), v)) + df(m + 10u));
    return vec2<f32>(s, t);
}

struct Taps {
    at: vec4<u32>,
    w: vec4<f32>,
}

// The four texels about a place, from the level's first, and their parts.
fn taps(size: vec2<i32>, bits: u32, uv: vec2<f32>) -> Taps {
    let at = vec2<f32>(keep(times(uv.x, f32(size.x)) - 0.5), keep(times(uv.y, f32(size.y)) - 0.5));
    let low = floor(at);
    let part = vec2<f32>(keep(at.x - low.x), keep(at.y - low.y));
    let x0 = wrap(i32(low.x), size.x, bits & 3u);
    let x1 = wrap(i32(low.x) + 1, size.x, bits & 3u);
    let y0 = wrap(i32(low.y), size.y, (bits >> 2u) & 3u) * size.x;
    let y1 = wrap(i32(low.y) + 1, size.y, (bits >> 2u) & 3u) * size.x;
    let a = keep(1.0 - part.x);
    let b = keep(1.0 - part.y);
    let w = vec4<f32>(times(a, b), times(part.x, b), times(a, part.y), times(part.x, part.y));
    return Taps(vec4<u32>(u32(y0 + x0), u32(y0 + x1), u32(y1 + x0), u32(y1 + x1)), w);
}

// A byte's 0..1.
fn byte(b: u32) -> f32 {
    return share(f32(b), 255.0);
}

fn display_linear(c: vec3<f32>) -> vec3<f32> {
    let above = pow(max(c + vec3<f32>(0.055), vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(above, c / 12.92, c <= vec3<f32>(0.04045));
}

fn texel_color(at: u32, srgb: bool) -> vec3<f32> {
    let c = texels[at];
    let b = vec3<u32>(c & 255u, (c >> 8u) & 255u, (c >> 16u) & 255u);
    if srgb {
        return vec3<f32>(df(p.linear_at + b.x), df(p.linear_at + b.y), df(p.linear_at + b.z));
    }
    return vec3<f32>(byte(b.x), byte(b.y), byte(b.z));
}

// `at` counts bytes.
fn texel_alpha(at: u32) -> f32 {
    return byte((texels[at >> 2u] >> (8u * (at & 3u))) & 255u);
}

// The alpha that is held against the cutoff: the solver's, of the weights `w`.
fn alpha(tri: u32, m: u32, w: vec3<f32>) -> f32 {
    let c = p.colors_at + faces[tri * FACE + 1u] * 12u;
    let a = times(blend(vec3<f32>(df(c + 3u), df(c + 7u), df(c + 11u)), w), df(m + 3u));
    let texture = data[m + 11u];
    if texture == NONE {
        return a;
    }
    let t = p.textures_at + texture * 8u;
    let k = taps(vec2<i32>(i32(data[t + 1u]), i32(data[t + 2u])), data[t + 6u], texture_at(tri, m, w));
    let at = k.at + vec4<u32>(data[t]);
    return times(a, mix4(k.w, vec4<f32>(texel_alpha(at.x), texel_alpha(at.y), texel_alpha(at.z), texel_alpha(at.w))));
}

// Asked of the second geometry alone. One that is no number stops on the CPU: tables.rs makes it opaque.
fn solid(tri: u32, w: vec3<f32>) -> bool {
    let m = material(tri);
    return alpha(tri, m, w) >= df(m + 7u);
}

fn albedo(tri: u32, m: u32, w: vec3<f32>) -> vec3<f32> {
    let f = p.colors_at + faces[tri * FACE + 1u] * 12u;
    var c = vec3<f32>(blend(vec3<f32>(df(f), df(f + 4u), df(f + 8u)), w), blend(vec3<f32>(df(f + 1u), df(f + 5u), df(f + 9u)), w), blend(vec3<f32>(df(f + 2u), df(f + 6u), df(f + 10u)), w));
    if (data[m + 12u] & DISPLAY_COLORS) != 0u {
        c = display_linear(c);
    }
    c = c * df3(m);
    let texture = data[m + 11u];
    if texture == NONE {
        return c;
    }
    let t = p.textures_at + texture * 8u;
    let bits = data[t + 6u];
    let k = taps(vec2<i32>(i32(data[t + 4u]), i32(data[t + 5u])), bits, texture_at(tri, m, w));
    let at = k.at + vec4<u32>(data[t + 3u]);
    let srgb = (bits & 16u) != 0u;
    let c0 = texel_color(at.x, srgb);
    let c1 = texel_color(at.y, srgb);
    let c2 = texel_color(at.z, srgb);
    let c3 = texel_color(at.w, srgb);
    return c * vec3<f32>(mix4(k.w, vec4<f32>(c0.x, c1.x, c2.x, c3.x)), mix4(k.w, vec4<f32>(c0.y, c1.y, c2.y, c3.y)), mix4(k.w, vec4<f32>(c0.z, c1.z, c2.z, c3.z)));
}

fn geometric(tri: u32) -> vec3<f32> {
    return ff3(tri * FACE + 2u);
}

// The interpolated normal, turned towards the ray on a double-sided back.
fn shading(tri: u32, flags: u32, w: vec3<f32>, front: bool) -> vec3<f32> {
    let f = tri * FACE;
    let a = ff3(f + 5u);
    let b = ff3(f + 8u);
    let c = ff3(f + 11u);
    let sum = vec3<f32>(blend(vec3<f32>(a.x, b.x, c.x), w), blend(vec3<f32>(a.y, b.y, c.y), w), blend(vec3<f32>(a.z, b.z, c.z), w));
    let size = length(sum);
    var n = geometric(tri);
    if size > 0.0 {
        n = sum * (1.0 / size);
    }
    if !front && (flags & DOUBLE_SIDED) != 0u {
        return -n;
    }
    return n;
}

fn lift(at: vec3<f32>) -> f32 {
    let a = abs(at);
    return max(0.01, times(4e-6, max(a.x, max(a.y, a.z))));
}

// On the triangle, as the solver has it.
fn on(tri: u32, w: vec3<f32>) -> vec3<f32> {
    let a = corner(tri, 0u);
    let b = corner(tri, 1u);
    let c = corner(tri, 2u);
    return vec3<f32>(blend(vec3<f32>(a.x, b.x, c.x), w), blend(vec3<f32>(a.y, b.y, c.y), w), blend(vec3<f32>(a.z, b.z, c.z), w));
}

// ---- Lights.

// A lamp's display-value light at a surface, nothing in the way (light.rs `Lamp::light`). The GPU may treat subnormal
// values (below f32::MIN_POSITIVE) as zero (WGSL allows it): a lamp's contribution below the smallest normal float, and
// a spot's direction within about 2^-63 units of its lamp, may then be zero here where the CPU has a subnormal value.
// The light differs by less than 1.2e-38 and a lamp or shadow-ray count may differ by such lamps; nothing visible
// changes (tables.rs `LEAST_RANGE`).
fn lamp_light(l: u32, at: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let range = lf(l + 3u);
    let d = lf3(l) - at;
    let d2 = blend(d, d);
    let rr = times(range, range);
    // Out of range before any quotient (the solver's 1 - d2 / rr is 0 there): of a tiny range the quotient is far past
    // what the correction of `quotient` can multiply.
    if !(range > 0.0 && rr > 0.0) || d2 >= rr {
        return vec3<f32>(0.0);
    }
    let reach = clamp(keep(1.0 - share(d2, rr)), 0.0, 1.0);
    if reach == 0.0 {
        return vec3<f32>(0.0);
    }
    // The cone's direction is the unit one from the lamp, near it too; none right at it. What faces the lamp is lit
    // by its distance over 0.01 there.
    var toward = vec3<f32>(0.0);
    if d2 > 0.0 {
        toward = -(d * (1.0 / sqrt(d2)));
    }
    let to = d * (1.0 / sqrt(max(d2, 1e-4)));
    let facing = clamp(blend(n, to), 0.0, 1.0);
    var cone = 1.0;
    if lights[l + 7u] != 0u {
        let c = clamp(keep(times(blend(lf3(l + 8u), toward), lf(l + 11u)) + lf(l + 12u)), 0.0, 1.0);
        cone = times(c, c);
    }
    return lf3(l + 4u) * times(times(reach, facing), cone);
}

// Ambient and directionals.
fn base(n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let at = lights[12];
    for (var k = 0u; k < lights[3]; k++) {
        let d = at + 6u * k;
        sum += lf3(d) * clamp(dot(n, lf3(d + 3u)), 0.0, 1.0);
    }
    return lf3(0u) + sum;
}

// max(s, 0)^2.2, of every positive sum as it is (light.rs `lit`): no pow of 0 or less, whose place takes a 1 that is
// not used. No sum is above 1e6 (tables.rs).
fn power(s: vec3<f32>) -> vec3<f32> {
    let none = s <= vec3<f32>(0.0);
    return select(pow(min(select(s, vec3<f32>(1.0), none), vec3<f32>(1e6)), vec3<f32>(2.2)), vec3<f32>(0.0), none);
}

fn lit(a: vec3<f32>, s: vec3<f32>) -> vec3<f32> {
    return min(a * power(s), vec3<f32>(1.0));
}

// The entries of the lamps that may reach a place: first, end, and the cell (light.rs `Grid::at`). On each axis
// c = (at - min) / side: outside of 0..=cells none; else the cell is c rounded down, and one at `cells` the last.
fn near(at: vec3<f32>) -> vec3<u32> {
    let low = lf3(4u);
    let side = lf(7u);
    let cells = vec3<u32>(lights[8], lights[9], lights[10]);
    var i = 0u;
    for (var k = 3u; k > 0u; k--) {
        let a = k - 1u;
        let c = share(keep(at[a] - low[a]), side);
        if !(c >= 0.0 && c <= f32(cells[a])) {
            return vec3<u32>(0u, 0u, NONE);
        }
        i = i * cells[a] + min(u32(c), cells[a] - 1u);
    }
    return vec3<u32>(lights[lights[14] + i], lights[lights[14] + i + 1u], i);
}

// ---- The solver's triangle test (nvf-rtgi bvh.rs `meet`), in its own order of operations.

struct Meeting {
    met: bool,
    t: f32,
    // The edges' functions and their sum: the weights are the quotients.
    w: vec3<f32>,
    sum: f32,
}

// The triangle and a ray from `o` along direction `r`.
fn meet(tri: u32, o: vec3<f32>, r: u32) -> Meeting {
    let at = p.directions_at + DIRECTION * r;
    let axes = data[at + AXES];
    let x = axes & 3u;
    let y = (axes >> 2u) & 3u;
    let z = (axes >> 4u) & 3u;
    let shear = df3(at + SHEAR);
    var c: array<vec3<f32>, 3>;
    for (var k = 0u; k < 3u; k++) {
        var v = corner(tri, k) - o;
        v = vec3<f32>(keep(v.x), keep(v.y), keep(v.z));
        c[k] = vec3<f32>(keep(v[x] - times(shear.x, v[z])), keep(v[y] - times(shear.y, v[z])), times(shear.z, v[z]));
    }
    var w = vec3<f32>(edge(c[2], c[1]), edge(c[0], c[2]), edge(c[1], c[0]));
    if any(w == vec3<f32>(0.0)) {
        w = vec3<f32>(fine(c[2], c[1]), fine(c[0], c[2]), fine(c[1], c[0]));
    }
    var out = Meeting(false, 0.0, w, keep(keep(w.x + w.y) + w.z));
    if (any(w < vec3<f32>(0.0)) && any(w > vec3<f32>(0.0))) || out.sum == 0.0 {
        return out;
    }
    let n = keep(keep(times(w.x, c[0].z) + times(w.y, c[1].z)) + times(w.z, c[2].z));
    if n != 0.0 {
        out.t = quotient(n, out.sum);
    }
    // Finite.
    out.met = (bitcast<u32>(out.t) & 0x7f800000u) != 0x7f800000u;
    return out;
}

// The corners' weights as the solver divides them: the alpha test of a candidate and the place of the winner alike.
fn shares(m: Meeting) -> vec3<f32> {
    return vec3<f32>(share(m.w.x, m.sum), share(m.w.y, m.sum), share(m.w.z, m.sum));
}

// Whether a ray from `o` along direction `r` enters the box of a list's triangle (nvf-rtgi bvh.rs `enter`): where a ray
// starts in a surface's plane the solver's tree leaves out what is behind it, though the test would meet it.
fn entered(at: u32, o: vec3<f32>, r: u32) -> bool {
    let d = p.directions_at + DIRECTION * r;
    let still = data[d + AXES] >> 6u;
    let inverse = df3(d + INVERSE);
    let least = vec3<f32>(bitcast<f32>(places[at + 1u]), bitcast<f32>(places[at + 2u]), bitcast<f32>(places[at + 3u]));
    let most = vec3<f32>(bitcast<f32>(places[at + 4u]), bitcast<f32>(places[at + 5u]), bitcast<f32>(places[at + 6u]));
    var first = 0.0;
    var last = 3e38;
    for (var k = 0u; k < 3u; k++) {
        if ((still >> k) & 1u) != 0u {
            if o[k] < least[k] || o[k] > most[k] {
                return false;
            }
            continue;
        }
        let a = times(keep(least[k] - o[k]), inverse[k]);
        let b = times(keep(most[k] - o[k]), inverse[k]);
        first = max(first, min(a, b));
        // 1 + 6e-7.
        last = min(last, times(max(a, b), bitcast<f32>(0x3f800005u)));
    }
    return first <= last;
}

// A shadow segment's direction as the solver's rays take it (bvh.rs `Ray::new`): a part below 1e-20 of the largest is 0.
// Its other rule, a part whose reciprocal overflows, is within this one here: a segment that is traced is longer than
// the lift, 0.01.
fn as_rays_go(d: vec3<f32>) -> vec3<f32> {
    let a = abs(d);
    return select(d, vec3<f32>(0.0), a < vec3<f32>(times(1e-20, max(a.x, max(a.y, a.z)))));
}

// Whether a triangle is one of a list's.
fn listed(tri: u32, list: vec2<u32>) -> bool {
    for (var k = 0u; k < list.y; k++) {
        if places[list.x + NEAR * k] == tri {
            return true;
        }
    }
    return false;
}

// ---- A point's rays.

// What the last ray of `gather` met, for `replay`.
struct Seen {
    // 1 hit, 2 hidden, 4 front, 8 unlit, 16 of the structure's nearest, of which the solver's test met nothing.
    flags: u32,
    tri: u32,
    t: f32,
    w: vec3<f32>,
    at: vec3<f32>,
    start: vec3<f32>,
    light: vec3<f32>,
    cell: u32,
    lamps: u32,
    shadows: u32,
    // The lamps in sight.
    free: u32,
}

struct Part {
    // C0, Cx, Cy, Cz; beside them the hits, the backs, the lamps of the lit hits' cells, the shadow rays.
    c0: vec4<f32>,
    cx: vec4<f32>,
    cy: vec4<f32>,
    cz: vec4<f32>,
}

var<private> seen: Seen;
var<workgroup> parts: array<Part, GROUP>;

// Of the rays from a point those from `first`, `step` apart; their sums. `list`: where the triangles near the point
// are (tables.rs `Points`), and how many. Without `shade` only the hits and the backs are counted. Both queries are
// declared here, outside every loop: naga 30 makes one that is declared inside a loop anew each time round, and takes
// none as a parameter.
fn gather(at: vec3<f32>, first: u32, step: u32, list: vec2<u32>, shade: bool) -> Part {
    var rq: ray_query;
    var sq: ray_query;
    var sum = Part(vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0));
    for (var r = first; r < p.rays; r += step) {
        // As the rays go for all they meet; as given for the moments.
        let dir = direction(r, ALONG);
        let given = direction(r, GIVEN);
        seen.flags = 0u;
        seen.light = vec3<f32>(0.0);
        var tri = NONE;
        var w = vec3<f32>(0.0);
        var t = 0.0;
        var plain = false;
        // The solver's nearest one so far: its number in the scene's order, its test.
        var order = NONE;
        var best = Meeting(false, 0.0, vec3<f32>(0.0), 0.0);
        var tri_best = NONE;
        // The triangles of the point's list are the solver's test's alone: at a distance of about 0, and at the edges
        // and corners there, the structure's has others.
        for (var i = 0u; i < list.y; i++) {
            let k = places[list.x + NEAR * i];
            if !entered(list.x + NEAR * i, at, r) {
                continue;
            }
            let m = meet(k, at, r);
            let first_of = faces[k * FACE + 20u];
            if m.met && m.t > 0.0 && (order == NONE || m.t < best.t || (m.t == best.t && first_of < order)) {
                if k < p.opaque || solid(k, shares(m)) {
                    best = m;
                    order = first_of;
                    tri_best = k;
                }
            }
        }
        // With such a list every candidate is looked at, for those in it to be passed by.
        rayQueryInitialize(&rq, scene, RayDesc(select(RAY_FLAG_NONE, RAY_FLAG_FORCE_NO_OPAQUE, list.y > 0u), 0xffu, p.near, FAR, at, dir));
        while rayQueryProceed(&rq) {
            let c = rayQueryGetCandidateIntersection(&rq);
            if c.kind == RAY_QUERY_INTERSECTION_TRIANGLE {
                let k = number(c.geometry_index, c.primitive_index);
                if !listed(k, list) && (k < p.opaque || solid(k, weights(c.barycentrics))) {
                    rayQueryConfirmIntersection(&rq);
                }
            }
        }
        let h = rayQueryGetCommittedIntersection(&rq);
        if h.kind != RAY_QUERY_INTERSECTION_NONE {
            tri = number(h.geometry_index, h.primitive_index);
            w = weights(h.barycentrics);
            t = h.t;
            // All that is in the place of the structure's nearest is tested, for what is nearest by the solver's test;
            // nothing is committed, so that every triangle there is a candidate. Those of the point's list were decided
            // above, with their leaves' boxes.
            let reach = min(WINDOW / max(abs(blend(geometric(tri), dir)), GRAZING), WIDEST);
            var met = false;
            rayQueryInitialize(&sq, scene, RayDesc(RAY_FLAG_FORCE_NO_OPAQUE, 0xffu, 0.0, 2.0 * reach, on(tri, w) - dir * reach, dir));
            while rayQueryProceed(&sq) {
                let c = rayQueryGetCandidateIntersection(&sq);
                if c.kind == RAY_QUERY_INTERSECTION_TRIANGLE {
                    let k = number(c.geometry_index, c.primitive_index);
                    if listed(k, list) {
                        continue;
                    }
                    let m = meet(k, at, r);
                    if m.met && m.t > 0.0 && (k < p.opaque || solid(k, shares(m))) {
                        met = true;
                        let first_of = faces[k * FACE + 20u];
                        if order == NONE || m.t < best.t || (m.t == best.t && first_of < order) {
                            best = m;
                            order = first_of;
                            tri_best = k;
                        }
                    }
                }
            }
            // Where the test meets none of them the structure's stays, if it is nearer than what is near the point.
            plain = !met && (order == NONE || t < best.t);
        } else if order == NONE {
            continue;
        }
        if order != NONE && !plain {
            tri = tri_best;
            w = shares(best);
            t = best.t;
        }
        sum.c0.w += 1.0;
        let m = material(tri);
        let flags = data[m + 12u];
        let front = blend(geometric(tri), dir) < 0.0;
        let hidden = ((flags & BACK_CULLED) != 0u && !front) || ((flags & FRONT_CULLED) != 0u && front);
        // On the triangle, as the solver has it, not along the ray.
        let spot = on(tri, w);
        let g = select(-geometric(tri), geometric(tri), front);
        let l = lift(spot);
        let start = vec3<f32>(keep(spot.x + times(g.x, l)), keep(spot.y + times(g.y, l)), keep(spot.z + times(g.z, l)));
        seen.flags = 1u | select(0u, 2u, hidden) | select(0u, 4u, front) | select(0u, 8u, (flags & UNLIT) != 0u) | select(0u, 16u, plain);
        seen.tri = tri;
        seen.t = t;
        seen.w = w;
        seen.at = spot;
        seen.start = start;
        seen.cell = NONE;
        seen.lamps = 0u;
        seen.shadows = 0u;
        seen.free = 0u;
        if hidden {
            sum.cx.w += 1.0;
            continue;
        }
        if !shade {
            continue;
        }
        var light = vec3<f32>(0.0);
        if (flags & UNLIT) != 0u {
            light = clamp(albedo(tri, m, w), vec3<f32>(0.0), vec3<f32>(1.0)) * p.emission;
        } else {
            let n = shading(tri, flags, w, front);
            let these = near(spot);
            var lamps = vec3<f32>(0.0);
            var shadows = 0u;
            for (var k = these.x; k < these.y; k++) {
                let lamp = lights[13] + lights[lights[15] + k] * LAMP;
                let add = lamp_light(lamp, spot, n);
                if all(add == vec3<f32>(0.0)) {
                    continue;
                }
                shadows += 1u;
                let d = lf3(lamp) - start;
                let end = 1.0 - lift(lf3(lamp)) / max(length(d), 1e-6);
                var free = true;
                // Stopped in the open (0, end) alone: after `near` and up to the last number below the end.
                if end > 1e-20 {
                    let last = bitcast<f32>(bitcast<u32>(end) - 1u);
                    rayQueryInitialize(&sq, scene, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0xffu, min(p.near, last), last, start, as_rays_go(d)));
                    while rayQueryProceed(&sq) {
                        let c = rayQueryGetCandidateIntersection(&sq);
                        if c.kind == RAY_QUERY_INTERSECTION_TRIANGLE {
                            if solid(number(c.geometry_index, c.primitive_index), weights(c.barycentrics)) {
                                rayQueryConfirmIntersection(&sq);
                            }
                        }
                    }
                    free = rayQueryGetCommittedIntersection(&sq).kind == RAY_QUERY_INTERSECTION_NONE;
                }
                if free {
                    lamps += add;
                    seen.free += 1u;
                }
            }
            sum.cy.w += f32(these.y - these.x);
            sum.cz.w += f32(shadows);
            seen.cell = these.z;
            seen.lamps = these.y - these.x;
            seen.shadows = shadows;
            if any(lamps != vec3<f32>(0.0)) {
                let a = albedo(tri, m, w);
                let under = base(n);
                light = max(lit(a, under + lamps) - lit(a, under), vec3<f32>(0.0));
            }
        }
        seen.light = light;
        sum.c0 += vec4<f32>(light, 0.0);
        sum.cx += vec4<f32>(light * (2.0 * given.x), 0.0);
        sum.cy += vec4<f32>(light * (2.0 * given.y), 0.0);
        sum.cz += vec4<f32>(light * (2.0 * given.z), 0.0);
    }
    return sum;
}

fn point_of(k: u32) -> u32 {
    return places[p.order_at + p.first + k];
}

fn position(point: u32) -> vec3<f32> {
    let i = point * PLACE;
    return vec3<f32>(bitcast<f32>(places[i]), bitcast<f32>(places[i + 1u]), bitcast<f32>(places[i + 2u]));
}

// Where its triangles' list is and how long (tables.rs `Points::words`).
fn near_list(point: u32) -> vec2<u32> {
    let i = point * PLACE + 5u;
    return vec2<u32>(places[i], places[i + 1u]);
}

// The workgroup's sums, the light's as means over all rays.
fn total() -> Part {
    var sum = parts[0];
    for (var i = 1u; i < GROUP; i++) {
        sum.c0 += parts[i].c0;
        sum.cx += parts[i].cx;
        sum.cy += parts[i].cy;
        sum.cz += parts[i].cz;
    }
    let each = vec4<f32>(vec3<f32>(quotient(1.0, f32(p.rays))), 1.0);
    return Part(sum.c0 * each, sum.cx * each, sum.cy * each, sum.cz * each);
}

fn put(o: u32, v: vec4<f32>) {
    out[o] = bitcast<u32>(v.x);
    out[o + 1u] = bitcast<u32>(v.y);
    out[o + 2u] = bitcast<u32>(v.z);
    out[o + 3u] = bitcast<u32>(v.w);
}

// C0, hits | Cx, backs | Cy, lamps | Cz, shadow rays: sixteen words a point. live.rs `probes_of`.
@compute @workgroup_size(64)
fn solve(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    let point = point_of(group.x);
    parts[local] = gather(position(point), local, GROUP, near_list(point), true);
    workgroupBarrier();
    if local == 0u {
        let sum = total();
        let o = MOMENTS * point;
        put(o, sum.c0);
        put(o + 4u, sum.cx);
        put(o + 8u, sum.cy);
        put(o + 12u, sum.cz);
    }
}

// One ray an invocation: ray i % rays of the batch's point i / rays. live.rs `Answer`.
@compute @workgroup_size(64)
fn replay(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.count * p.rays {
        return;
    }
    let point = point_of(id.x / p.rays);
    let sum = gather(position(point), id.x % p.rays, p.rays, near_list(point), true);
    let o = (p.first * p.rays + id.x) * ANSWER;
    out[o] = seen.flags;
    out[o + 1u] = seen.tri;
    out[o + 2u] = bitcast<u32>(seen.t);
    put(o + 3u, vec4<f32>(seen.w, seen.at.x));
    put(o + 7u, vec4<f32>(seen.at.yz, seen.start.xy));
    out[o + 11u] = bitcast<u32>(seen.start.z);
    // What the sum got, so that this is the solve's own light.
    put(o + 12u, vec4<f32>(sum.c0.xyz, 0.0));
    out[o + 15u] = seen.cell;
    out[o + 16u] = seen.lamps;
    out[o + 17u] = seen.shadows;
    out[o + 18u] = seen.free;
    out[o + 19u] = 1u;
}

// A point's hits and, of them, the backs: two words a point. live.rs `reach_of`.
@compute @workgroup_size(64)
fn reach(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    let point = point_of(group.x);
    parts[local] = gather(position(point), local, GROUP, near_list(point), false);
    workgroupBarrier();
    if local == 0u {
        var hits = 0u;
        var backs = 0u;
        for (var i = 0u; i < GROUP; i++) {
            hits += u32(parts[i].c0.w);
            backs += u32(parts[i].cx.w);
        }
        out[2u * point] = hits;
        out[2u * point + 1u] = backs;
    }
}

// The solver's triangle test of the batch's point i against primitive i, along direction i % rays. live.rs `Meeting`.
@compute @workgroup_size(64)
fn meetings(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= p.count {
        return;
    }
    let point = point_of(id.x);
    let m = meet(point, position(point), point % p.rays);
    let o = point * MEETING;
    out[o] = u32(m.met);
    out[o + 1u] = bitcast<u32>(m.t);
    put(o + 2u, vec4<f32>(m.w, share(m.w.x, m.sum)));
    out[o + 6u] = bitcast<u32>(share(m.w.y, m.sum));
    out[o + 7u] = bitcast<u32>(share(m.w.z, m.sum));
}
