//! The zombie's body as a signed distance field of a human, turned into smooth skinned meshes once at startup.
//!
//! Everything is described in a rest pose with the arms held out a little from the sides (so they don't melt into
//! the ribs), standing at the origin, facing -z, one unit a metre. The field is sampled on a grid, a surface-nets
//! pass turns it into a mesh, and every vertex gets weights for the bones of the skeleton the renderer poses.

use crate::sim::V3;

/// Bones, in the order the renderer passes their matrices. Paired bones come left (x < 0) then right.
pub const BONES: usize = 17;
pub const PELVIS: usize = 0;
pub const SPINE: usize = 1;
pub const CHEST: usize = 2;
pub const HEAD: usize = 3;
pub const JAW: usize = 4;
pub const HIP: usize = 5;
pub const KNEE: usize = 7;
pub const ANKLE: usize = 9;
pub const SHOULDER: usize = 11;
pub const ELBOW: usize = 13;
pub const WRIST: usize = 15;

/// Floats per vertex: position, normal, four bone indices, four weights, and one number for the shader (how far
/// down the arm a vertex is, or 9 when it is not on one).
pub const STRIDE: usize = 15;

/// Where the arms hang from, and how far out they are held in the rest pose.
pub const ARM_X: f32 = 0.205;
pub const SHOULDER_Y: f32 = 1.41;
pub const ARM_ANGLE: f32 = 0.5;
/// The head is cut from the body here, and the hands where the forearm ends.
const NECK_Y: f32 = 1.50;
const WRIST_Y: f32 = 0.875;
/// The face is modelled a size too big (the eyes, teeth and face paint are placed in that model's frame): it is
/// shrunk by `HEAD_K` about the middle of the neck at `HEAD_PIVOT` and lifted by `HEAD_LIFT`, so the head is about
/// an eighth of the height and sits on a neck. The zombie shader paints the face with a copy of these three.
pub const HEAD_K: f32 = 0.87;
pub const HEAD_PIVOT: f32 = 1.50;
pub const HEAD_LIFT: f32 = 0.025;

/// A point of the body to where it is on the face as modelled.
fn to_head(p: V) -> V {
    [p[0] / HEAD_K, HEAD_PIVOT + (p[1] - HEAD_PIVOT - HEAD_LIFT) / HEAD_K, p[2] / HEAD_K]
}

/// A point on the face as modelled to where it is on the body.
pub fn head_to_rest(p: V) -> V {
    [p[0] * HEAD_K, HEAD_PIVOT + HEAD_LIFT + (p[1] - HEAD_PIVOT) * HEAD_K, p[2] * HEAD_K]
}

pub struct Mesh {
    pub verts: Vec<f32>,
    pub idx: Vec<u32>,
}

type V = V3;

pub(crate) fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn len(a: V) -> f32 {
    dot(a, a).sqrt()
}

/// Union of two shapes with the join rounded off over a distance `k`.
pub(crate) fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// `a` with `b` carved out of it, rounded over `k`.
fn smooth_sub(a: f32, b: f32, k: f32) -> f32 {
    -smin(-a, b, k)
}

/// A cone with rounded ends: radius `r1` at `a`, `r2` at `b`.
pub(crate) fn cone(p: V, a: V, b: V, r1: f32, r2: f32) -> f32 {
    let ba = sub(b, a);
    let l2 = dot(ba, ba);
    let rr = r1 - r2;
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = sub(p, a);
    let y = dot(pa, ba);
    let z = y - l2;
    let w = [pa[0] * l2 - ba[0] * y, pa[1] * l2 - ba[1] * y, pa[2] * l2 - ba[2] * y];
    let x2 = dot(w, w);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = rr.signum() * rr * rr * x2;
    if z.signum() * a2 * z2 > k {
        (x2 + z2).sqrt() * il2 - r2
    } else if y.signum() * a2 * y2 < k {
        (x2 + y2).sqrt() * il2 - r1
    } else {
        ((x2 * a2 * il2).sqrt() + y * rr) * il2 - r1
    }
}

/// An ellipsoid of half-sizes `r` centred on `c` (a good bound on the distance, not the exact distance).
pub(crate) fn ell(p: V, c: V, r: V) -> f32 {
    let q = [(p[0] - c[0]) / r[0], (p[1] - c[1]) / r[1], (p[2] - c[2]) / r[2]];
    let k0 = len(q);
    let k1 = len([q[0] / r[0], q[1] / r[1], q[2] / r[2]]);
    if k1 < 1e-6 {
        -r[0].min(r[1]).min(r[2])
    } else {
        k0 * (k0 - 1.0) / k1
    }
}

/// A cone whose cross-section is squashed front to back to `flat` of its width.
fn flat_cone(p: V, a: V, b: V, r1: f32, r2: f32, flat: f32) -> f32 {
    cone([p[0], p[1], p[2] / flat], [a[0], a[1], a[2] / flat], [b[0], b[1], b[2] / flat], r1, r2) * flat
}

/// A point in the rest pose, in the frame of the arm hanging straight down from the right shoulder.
fn arm_down(p: V) -> V {
    let (sn, cs) = ARM_ANGLE.sin_cos();
    let q = [p[0].abs() - ARM_X, p[1] - SHOULDER_Y];
    [ARM_X + cs * q[0] + sn * q[1], SHOULDER_Y - sn * q[0] + cs * q[1], p[2]]
}

/// The other way: a point on the right arm hanging down, to where it is in the rest pose.
pub fn arm_rest(side: f32, p: V) -> V {
    let (sn, cs) = ARM_ANGLE.sin_cos();
    let q = [p[0] - ARM_X, p[1] - SHOULDER_Y];
    [side * (ARM_X + cs * q[0] - sn * q[1]), SHOULDER_Y + sn * q[0] + cs * q[1], p[2]]
}

/// Neck to hips: a ribcage broad across the chest narrowing to the waist (the V of a man's back), a pelvis no
/// wider than the chest, shoulders that slope from the neck out to the arms.
fn torso(p: V) -> f32 {
    let ax = [p[0].abs(), p[1], p[2]];
    // ribcage, belly and pelvis
    let mut d = ell(p, [0.0, 1.265, 0.008], [0.152, 0.2, 0.112]);
    d = smin(d, flat_cone(p, [0.0, 1.13, 0.01], [0.0, 0.99, 0.008], 0.132, 0.138, 0.74), 0.07);
    d = smin(d, ell(p, [0.0, 1.06, -0.035], [0.115, 0.1, 0.075]), 0.05);
    d = smin(d, ell(p, [0.0, 0.935, 0.01], [0.158, 0.1, 0.105]), 0.06);
    // the broad back under the arms, the shoulder blades, and the buttocks
    d = smin(d, ell(ax, [0.1, 1.27, 0.035], [0.075, 0.15, 0.072]), 0.05);
    d = smin(d, ell(ax, [0.075, 1.355, 0.072], [0.068, 0.075, 0.045]), 0.04);
    d = smin(d, ell(ax, [0.068, 0.875, 0.055], [0.08, 0.085, 0.07]), 0.04);
    // chest muscles: broad flat plates
    d = smin(d, ell(ax, [0.075, 1.335, -0.08], [0.09, 0.055, 0.032]), 0.05);
    // the mass between the ribs and the arm, then the trapezius sloping from the neck to the shoulder
    d = smin(d, ell(ax, [0.15, 1.39, 0.0], [0.07, 0.055, 0.065]), 0.04);
    d = smin(d, cone(ax, [0.025, 1.5, 0.025], [0.17, 1.435, 0.012], 0.048, 0.034), 0.045);
    // collarbones, the neck and its two strap muscles
    d = smin(d, cone(ax, [0.015, 1.443, -0.06], [0.165, 1.452, -0.028], 0.011, 0.011), 0.02);
    d = smin(d, cone(p, [0.0, 1.43, 0.012], [0.0, 1.62, -0.004], 0.06, 0.053), 0.04);
    d = smin(d, cone(ax, [0.045, 1.6, 0.012], [0.018, 1.455, -0.052], 0.015, 0.013), 0.02);
    d
}

/// Both legs, hips to toes: thighs that taper to the knee, a kneecap, calves at the back of the shin, slim ankles.
fn legs(p: V) -> f32 {
    let q = [p[0].abs(), p[1], p[2]];
    let mut d = cone(q, [0.092, 0.92, 0.005], [0.085, 0.5, -0.004], 0.09, 0.052);
    // the front, outside and back of the thigh
    d = smin(d, ell(q, [0.09, 0.72, -0.022], [0.07, 0.17, 0.066]), 0.04);
    d = smin(d, ell(q, [0.118, 0.76, 0.0], [0.045, 0.14, 0.06]), 0.04);
    d = smin(d, ell(q, [0.085, 0.72, 0.028], [0.06, 0.15, 0.058]), 0.04);
    // the knee and its cap
    d = smin(d, ell(q, [0.085, 0.5, -0.01], [0.046, 0.06, 0.046]), 0.04);
    d = smin(d, ell(q, [0.085, 0.497, -0.044], [0.027, 0.032, 0.018]), 0.025);
    // the shin, the calf behind it and the shinbone down the front
    d = smin(d, cone(q, [0.085, 0.49, 0.0], [0.08, 0.095, 0.004], 0.048, 0.031), 0.03);
    d = smin(d, ell(q, [0.087, 0.37, 0.024], [0.048, 0.11, 0.052]), 0.04);
    d = smin(d, ell(q, [0.075, 0.34, 0.022], [0.04, 0.08, 0.04]), 0.03);
    d = smin(d, cone(q, [0.085, 0.45, -0.038], [0.08, 0.14, -0.026], 0.014, 0.011), 0.02);
    // ankle, heel, foot and toes
    d = smin(d, ell(q, [0.08, 0.085, 0.0], [0.037, 0.045, 0.038]), 0.02);
    d = smin(d, ell(q, [0.08, 0.04, 0.028], [0.034, 0.04, 0.045]), 0.02);
    d = smin(d, ell(q, [0.083, 0.042, -0.06], [0.045, 0.038, 0.115]), 0.02);
    d = smin(d, ell(q, [0.087, 0.026, -0.155], [0.044, 0.025, 0.045]), 0.015);
    d
}

/// One hand in the arm's own frame: a palm, a thumb and four curled fingers.
fn hand(q: V) -> f32 {
    let mut d = ell(q, [ARM_X, 0.818, 0.0], [0.023, 0.058, 0.042]);
    d = smin(d, cone(q, [ARM_X - 0.012, 0.855, -0.028], [ARM_X - 0.042, 0.79, -0.054], 0.013, 0.01), 0.012);
    for (i, dz) in [-0.029f32, -0.0095, 0.0095, 0.028].iter().enumerate() {
        let longer = if i == 1 || i == 2 { 0.009 } else if i == 3 { -0.01 } else { 0.0 };
        let (a, b, c) = ([ARM_X, 0.775, *dz], [ARM_X - 0.014, 0.728 - longer, *dz], [ARM_X - 0.036, 0.695 - longer, *dz]);
        d = smin(d, cone(q, a, b, 0.0105, 0.0093), 0.006);
        d = smin(d, cone(q, b, c, 0.0093, 0.0078), 0.006);
    }
    d
}

/// The whole arm, shoulder to fingertips, at `p` in the rest pose: a deltoid capping the shoulder, biceps and
/// triceps, a forearm thick below the elbow that thins to a flat wrist.
fn arm(p: V) -> f32 {
    let q = arm_down(p);
    let mut d = ell(q, [ARM_X + 0.006, 1.37, 0.0], [0.054, 0.085, 0.06]);
    d = smin(d, cone(q, [ARM_X, 1.40, 0.0], [ARM_X, 1.13, 0.0], 0.046, 0.036), 0.03);
    d = smin(d, ell(q, [ARM_X, 1.255, -0.016], [0.04, 0.085, 0.04]), 0.03);
    d = smin(d, ell(q, [ARM_X + 0.004, 1.28, 0.02], [0.04, 0.095, 0.04]), 0.03);
    d = smin(d, ell(q, [ARM_X, 1.13, 0.006], [0.036, 0.04, 0.038]), 0.02);
    // the forearm is broader across the back of the hand than through it
    let flat = [q[0], q[1], q[2] * 0.78];
    d = smin(d, cone(flat, [ARM_X, 1.12, 0.0], [ARM_X, WRIST_Y, 0.0], 0.034, 0.024), 0.025);
    d = smin(d, ell(q, [ARM_X, 1.055, -0.004], [0.042, 0.085, 0.043]), 0.035);
    smin(d, hand(q), 0.015)
}

/// Skull, face, jaw and ears of a zombie. The face is built up from simple shapes and then carved: eye sockets,
/// hollow cheeks, an angry squint and an open mouth.
fn head(p: V) -> f32 {
    let q = [p[0].abs(), p[1], p[2]];
    let mut d = ell(p, [0.0, 1.68, 0.012], [0.098, 0.118, 0.108]);
    d = smin(d, ell(p, [0.0, 1.62, -0.028], [0.088, 0.088, 0.084]), 0.02);
    // cheekbones and the hollows under them, brow ridge, nose, ears
    d = smin(d, ell(q, [0.053, 1.638, -0.07], [0.032, 0.024, 0.026]), 0.015);
    d = smin(d, cone(q, [0.008, 1.683, -0.092], [0.064, 1.702, -0.068], 0.014, 0.011), 0.014);
    d = smin(d, cone(p, [0.0, 1.665, -0.094], [0.0, 1.612, -0.112], 0.008, 0.013), 0.012);
    d = smin(d, ell(q, [0.015, 1.604, -0.1], [0.011, 0.008, 0.01]), 0.01);
    d = smin(d, ell(p, [0.0, 1.605, -0.114], [0.012, 0.011, 0.012]), 0.01);
    d = smin(d, ell(q, [0.097, 1.655, 0.012], [0.01, 0.028, 0.018]), 0.01);
    // jaw and chin
    d = smin(d, cone(q, [0.047, 1.60, 0.0], [0.0, 1.535, -0.07], 0.034, 0.028), 0.02);
    d = smin(d, ell(p, [0.0, 1.533, -0.079], [0.031, 0.024, 0.026]), 0.014);
    d = smin(d, ell(p, [0.0, 1.592, -0.094], [0.029, 0.0075, 0.013]), 0.008);
    d = smin(d, ell(p, [0.0, 1.558, -0.092], [0.027, 0.0085, 0.013]), 0.008);
    // hollow cheeks
    d = smooth_sub(d, ell(q, [0.075, 1.6, -0.055], [0.016, 0.024, 0.02]), 0.035);
    // eye sockets and the open mouth
    d = smooth_sub(d, ell(q, [0.04, 1.668, -0.084], [0.021, 0.016, 0.02]), 0.008);
    // lids close over the top and bottom of each eye, leaving an angry squint
    d = smin(d, ell(q, [0.04, 1.681, -0.088], [0.0165, 0.0055, 0.013]), 0.004);
    d = smin(d, ell(q, [0.04, 1.6615, -0.089], [0.0145, 0.0035, 0.011]), 0.004);
    d = smooth_sub(d, ell(p, [0.0, 1.574, -0.083], [0.023, 0.0075, 0.03]), 0.006);
    d
}

/// A living head: the same skull, with a face built on the bones under it rather than on a zombie's lumps. A
/// mid-face that narrows to the mouth, cheekbones running back to the ears, a jaw from below the ear to a squared
/// chin, closed lips on the curve of the teeth, and open eyes under a relaxed lid.
fn living_head(p: V) -> f32 {
    let q = [p[0].abs(), p[1], p[2]];
    let mut d = ell(p, [0.0, 1.68, 0.012], [0.098, 0.118, 0.108]);
    d = smin(d, ell(p, [0.0, 1.64, -0.025], [0.078, 0.07, 0.07]), 0.025);
    // a soft brow, cheekbones from under the eye back toward the ear, the cheeks over the teeth, the flesh down
    // the side of the face to the jaw
    d = smin(d, ell(q, [0.042, 1.695, -0.084], [0.028, 0.01, 0.015]), 0.02);
    d = smin(d, ell(p, [0.0, 1.69, -0.09], [0.015, 0.01, 0.01]), 0.015);
    d = smin(d, cone(q, [0.042, 1.648, -0.078], [0.077, 1.645, -0.025], 0.012, 0.009), 0.02);
    d = smin(d, ell(q, [0.056, 1.638, -0.068], [0.022, 0.016, 0.018]), 0.025);
    d = smin(d, ell(q, [0.044, 1.626, -0.058], [0.028, 0.028, 0.022]), 0.04);
    d = smin(d, ell(q, [0.045, 1.608, -0.047], [0.02, 0.03, 0.028]), 0.03);
    // the mouth and chin are bent back round the curve of the teeth, the corners of the mouth well behind its
    // middle; the teeth themselves under the lips
    let b = [p[0], p[1], p[2] - 18.0 * p[0] * p[0]];
    d = smin(d, ell(b, [0.0, 1.595, -0.068], [0.036, 0.026, 0.022]), 0.03);
    // the nose, ears
    d = smin(d, cone(p, [0.0, 1.668, -0.094], [0.0, 1.627, -0.107], 0.007, 0.011), 0.012);
    d = smin(d, ell(q, [0.0135, 1.617, -0.097], [0.0095, 0.007, 0.009]), 0.01);
    d = smin(d, ell(p, [0.0, 1.621, -0.108], [0.0105, 0.0095, 0.011]), 0.01);
    d = smin(d, ell(q, [0.097, 1.655, 0.012], [0.01, 0.028, 0.018]), 0.01);
    // the jaw: up from its angle to below the ear, along to the chin
    d = smin(d, cone(q, [0.055, 1.595, 0.002], [0.061, 1.625, 0.006], 0.014, 0.012), 0.03);
    d = smin(d, cone(q, [0.055, 1.595, 0.002], [0.016, 1.547, -0.074], 0.014, 0.012), 0.03);
    d = smin(d, ell(b, [0.0, 1.567, -0.074], [0.02, 0.02, 0.015]), 0.02);
    d = smin(d, ell(b, [0.0, 1.552, -0.085], [0.019, 0.015, 0.014]), 0.02);
    // closed lips, the upper one in two curves (the shader draws the line between them), and the eyes
    let bq = [b[0].abs(), b[1], b[2]];
    d = smin(d, ell(bq, [0.007, 1.5985, -0.089], [0.015, 0.006, 0.011]), 0.008);
    d = smin(d, ell(b, [0.0, 1.5805, -0.0855], [0.02, 0.009, 0.012]), 0.008);
    d = smooth_sub(d, ell(q, [0.04, 1.668, -0.084], [0.019, 0.014, 0.018]), 0.008);
    smin(d, ell(q, [0.04, 1.678, -0.0885], [0.0155, 0.0045, 0.0115]), 0.004)
}

/// The whole body, hands and head included, in the rest pose.
pub fn skin(p: V) -> f32 {
    figure(p, false)
}

/// The same body with a living face.
fn human_skin(p: V) -> f32 {
    figure(p, true)
}

fn figure(p: V, human: bool) -> f32 {
    let mut d = 1.0f32;
    if p[1] > 0.68 && p[1] < 1.64 && p[0].abs() < 0.32 {
        d = torso(p);
        if human {
            // the throat set back under the jaw, so the chin stands clear of it
            d = smooth_sub(d, ell(p, [0.0, 1.552, -0.09], [0.06, 0.036, 0.056]), 0.025);
        }
    }
    if p[1] < 1.12 {
        d = smin(d, legs(p), 0.045);
    }
    if p[1] > 1.36 {
        let h = to_head(p);
        d = smin(d, if human { living_head(h) } else { head(h) } * HEAD_K, 0.03);
    }
    if p[0].abs() > 0.1 && p[1] > 0.55 && p[1] < 1.62 {
        d = smin(d, arm(p), 0.02);
    }
    d
}

/// Negative inside the hands, from the wrist down.
fn hand_zone(p: V) -> f32 {
    let q = arm_down(p);
    (q[1] - WRIST_Y).max(((q[0] - ARM_X).powi(2) + q[2] * q[2]).sqrt() - 0.14)
}

/// The skin of the body from the neck down to the wrists (no head, no hands).
fn body_part(p: V) -> f32 {
    skin(p).max(p[1] - NECK_Y).max(-hand_zone(p))
}

fn head_part(p: V) -> f32 {
    skin(p).max(NECK_Y - p[1])
}

fn face_part(p: V) -> f32 {
    human_skin(p).max(NECK_Y - p[1])
}

fn hand_part(p: V) -> f32 {
    skin(p).max(hand_zone(p))
}

/// A shirt: the body from the hips to the neck and out along the arms, swollen to cloth thickness, that hangs
/// straight down from the chest and shoulder blades instead of clinging to the waist.
fn shirt(p: V) -> f32 {
    let hang = flat_cone(p, [0.0, 1.3, 0.012], [0.0, 0.86, 0.012], 0.158, 0.168, 0.75);
    let d = smin(body_part(p) - 0.013, hang.max(p[1] - 1.36), 0.04);
    d.max(0.84 - p[1]).max(p[1] - 1.545)
}

/// Trousers and shoes: the hips and legs swollen, each leg hanging loose from the seat to the ankle, a fatter
/// boot over the foot.
fn pants(p: V) -> f32 {
    let q = [p[0].abs(), p[1], p[2]];
    let mut d = smin(ell(p, [0.0, 0.935, 0.01], [0.158, 0.1, 0.105]), legs(p), 0.045);
    d = smin(d, ell(q, [0.068, 0.875, 0.055], [0.08, 0.085, 0.07]), 0.04) - 0.014;
    let leg = cone(q, [0.092, 0.86, 0.006], [0.083, 0.13, 0.004], 0.1, 0.058);
    d = smin(d, leg, 0.03);
    // shoes: a low upper over the foot and round the ankle, on a flat sole
    let shoe = smin(ell(q, [0.084, 0.05, -0.058], [0.05, 0.05, 0.13]), ell(q, [0.081, 0.085, 0.0], [0.045, 0.055, 0.048]), 0.03);
    d = smin(d, shoe.max(-p[1]), 0.02);
    d.max(p[1] - 1.04)
}

/// Hair on the skull and a beard on the jaw, as one thin shell; the shader decides which parts show.
fn hair(p: V) -> f32 {
    let h = to_head(p);
    let q = [h[0].abs(), h[1], h[2]];
    let mut d = ell(h, [0.0, 1.695, 0.03], [0.112, 0.126, 0.122]);
    d = smin(d, cone(q, [0.047, 1.60, 0.0], [0.0, 1.535, -0.07], 0.047, 0.04), 0.03);
    (d * HEAD_K).max(NECK_Y - p[1])
}

/// Close-cropped hair for the living: a thin shell over the skull.
fn crop(p: V) -> f32 {
    let h = to_head(p);
    (ell(h, [0.0, 1.686, 0.017], [0.104, 0.124, 0.115]) * HEAD_K).max(NECK_Y - p[1])
}

/// Where each bone runs in the rest pose, for the weights.
struct Seg {
    bone: usize,
    a: V,
    b: V,
    r: f32,
}

fn segments() -> Vec<Seg> {
    let mut s = vec![
        Seg { bone: PELVIS, a: [0.0, 0.88, 0.0], b: [0.0, 1.0, 0.0], r: 0.15 },
        Seg { bone: SPINE, a: [0.0, 1.0, 0.0], b: [0.0, 1.17, 0.0], r: 0.13 },
        Seg { bone: CHEST, a: [0.0, 1.17, 0.0], b: [0.0, 1.44, 0.0], r: 0.15 },
        Seg { bone: HEAD, a: head_to_rest([0.0, 1.52, 0.0]), b: head_to_rest([0.0, 1.76, 0.0]), r: 0.09 * HEAD_K },
        Seg { bone: JAW, a: head_to_rest([0.0, 1.585, -0.02]), b: head_to_rest([0.0, 1.54, -0.07]), r: 0.035 * HEAD_K },
    ];
    for (k, side) in [-1.0f32, 1.0].iter().enumerate() {
        let x = |v: f32| side * v;
        s.push(Seg { bone: HIP + k, a: [x(0.092), 0.92, 0.0], b: [x(0.085), 0.5, 0.0], r: 0.09 });
        s.push(Seg { bone: KNEE + k, a: [x(0.085), 0.5, 0.0], b: [x(0.08), 0.095, 0.0], r: 0.05 });
        s.push(Seg { bone: ANKLE + k, a: [x(0.08), 0.093, 0.0], b: [x(0.085), 0.04, -0.15], r: 0.04 });
        s.push(Seg { bone: SHOULDER + k, a: arm_rest(*side, [ARM_X, 1.40, 0.0]), b: arm_rest(*side, [ARM_X, 1.13, 0.0]), r: 0.055 });
        s.push(Seg { bone: ELBOW + k, a: arm_rest(*side, [ARM_X, 1.13, 0.0]), b: arm_rest(*side, [ARM_X, WRIST_Y, 0.0]), r: 0.04 });
        s.push(Seg { bone: WRIST + k, a: arm_rest(*side, [ARM_X, WRIST_Y, 0.0]), b: arm_rest(*side, [ARM_X, 0.715, 0.0]), r: 0.035 });
    }
    s
}

fn dist_to_segment(p: V, a: V, b: V) -> f32 {
    let (ab, ap) = (sub(b, a), sub(p, a));
    let t = (dot(ap, ab) / dot(ab, ab)).clamp(0.0, 1.0);
    len([ap[0] - ab[0] * t, ap[1] - ab[1] * t, ap[2] - ab[2] * t])
}

/// The four bones that move a point the most, with weights adding up to one.
fn weights(p: V, segs: &[Seg]) -> ([f32; 4], [f32; 4]) {
    let side_left = p[0] < 0.0;
    let mut cand: Vec<(usize, f32)> = Vec::new();
    for s in segs {
        // paired bones only matter on their own side, near the middle either will do
        let paired = (HIP..BONES).contains(&s.bone);
        if paired && ((s.bone - HIP) % 2 == 0) != side_left && p[0].abs() > 0.03 {
            continue;
        }
        let h = to_head(p);
        if s.bone == JAW && !(h[2] < -0.015 && h[1] < 1.605 && p[1] > NECK_Y) {
            continue;
        }
        let d = (dist_to_segment(p, s.a, s.b) - s.r).max(0.0);
        cand.push((s.bone, d));
    }
    cand.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let near = cand[0].1;
    let mut idx = [0.0f32; 4];
    let mut w = [0.0f32; 4];
    let mut sum = 0.0;
    for (i, (bone, d)) in cand.iter().take(4).enumerate() {
        idx[i] = *bone as f32;
        w[i] = (-(d - near) / 0.02).exp();
        sum += w[i];
    }
    for x in &mut w {
        *x /= sum;
    }
    (idx, w)
}

/// Surface nets: samples `f` on a grid of spacing `h` over the box `lo..hi`, only near the surface, and builds a
/// smooth mesh of where it crosses zero. Returns triangles over vertices at (position, normal).
pub(crate) fn surface_nets(f: &dyn Fn(V) -> f32, lo: V, hi: V, h: f32) -> (Vec<V>, Vec<V>, Vec<u32>) {
    let n = [0, 1, 2].map(|a| ((hi[a] - lo[a]) / h).ceil() as usize + 1);
    let at = |i: usize, j: usize, k: usize| i + n[0] * (j + n[1] * k);
    let pos = |i: usize, j: usize, k: usize| [lo[0] + i as f32 * h, lo[1] + j as f32 * h, lo[2] + k as f32 * h];
    let mut field = vec![f32::NAN; n[0] * n[1] * n[2]];

    // blocks of 8 cells: sample one only if the surface could pass through it
    const B: usize = 8;
    let blocks = [0, 1, 2].map(|a| (n[a] + B - 1) / B);
    let reach = (B as f32 * h) * 0.5 * 3f32.sqrt() * 1.35 + h;
    for bk in 0..blocks[2] {
        for bj in 0..blocks[1] {
            for bi in 0..blocks[0] {
                let c = [lo[0] + ((bi * B) as f32 + B as f32 / 2.0) * h, lo[1] + ((bj * B) as f32 + B as f32 / 2.0) * h, lo[2] + ((bk * B) as f32 + B as f32 / 2.0) * h];
                if f(c).abs() > reach {
                    continue;
                }
                for k in bk * B..(bk * B + B + 1).min(n[2]) {
                    for j in bj * B..(bj * B + B + 1).min(n[1]) {
                        for i in bi * B..(bi * B + B + 1).min(n[0]) {
                            let s = at(i, j, k);
                            if field[s].is_nan() {
                                field[s] = f(pos(i, j, k));
                            }
                        }
                    }
                }
            }
        }
    }

    // one vertex in every cell the surface crosses, at the mean of where it crosses the cell's edges
    const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];
    let corner = |c: usize| [c & 1, (c >> 1) & 1, (c >> 2) & 1];
    let mut cell = vec![u32::MAX; n[0] * n[1] * n[2]];
    let mut points: Vec<V> = Vec::new();
    for k in 0..n[2] - 1 {
        for j in 0..n[1] - 1 {
            for i in 0..n[0] - 1 {
                let mut d = [0.0f32; 8];
                let mut ok = true;
                for c in 0..8 {
                    let o = corner(c);
                    let v = field[at(i + o[0], j + o[1], k + o[2])];
                    if v.is_nan() {
                        ok = false;
                        break;
                    }
                    d[c] = v;
                }
                if !ok || d.iter().all(|v| *v > 0.0) || d.iter().all(|v| *v <= 0.0) {
                    continue;
                }
                let (mut sum, mut cnt) = ([0.0f32; 3], 0.0);
                for (a, b) in EDGES {
                    if (d[a] > 0.0) != (d[b] > 0.0) {
                        let t = d[a] / (d[a] - d[b]);
                        let (oa, ob) = (corner(a), corner(b));
                        for ax in 0..3 {
                            sum[ax] += oa[ax] as f32 + (ob[ax] as f32 - oa[ax] as f32) * t;
                        }
                        cnt += 1.0;
                    }
                }
                let base = pos(i, j, k);
                cell[at(i, j, k)] = points.len() as u32;
                points.push([base[0] + sum[0] / cnt * h, base[1] + sum[1] / cnt * h, base[2] + sum[2] / cnt * h]);
            }
        }
    }

    // a quad across every grid edge the surface crosses, joining the four cells around it
    let mut idx = Vec::new();
    for k in 1..n[2] - 1 {
        for j in 1..n[1] - 1 {
            for i in 1..n[0] - 1 {
                let here = field[at(i, j, k)];
                if here.is_nan() {
                    continue;
                }
                for axis in 0..3 {
                    let mut nb = [i, j, k];
                    nb[axis] += 1;
                    let there = field[at(nb[0], nb[1], nb[2])];
                    if there.is_nan() || (here > 0.0) == (there > 0.0) {
                        continue;
                    }
                    // the four cells that share this edge
                    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                    let mut q = [0u32; 4];
                    for (m, (du, dv)) in [(0, 0), (1, 0), (1, 1), (0, 1)].iter().enumerate() {
                        let mut c = [i, j, k];
                        c[u] -= 1 - du;
                        c[v] -= 1 - dv;
                        q[m] = cell[at(c[0], c[1], c[2])];
                    }
                    if q.contains(&u32::MAX) {
                        continue;
                    }
                    if here > 0.0 {
                        idx.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
                    } else {
                        idx.extend_from_slice(&[q[0], q[2], q[1], q[0], q[3], q[2]]);
                    }
                }
            }
        }
    }

    // settle each vertex onto the surface and take its normal from the field
    let e = h * 0.25;
    let mut normals = Vec::with_capacity(points.len());
    for p in &mut points {
        for _ in 0..2 {
            let g = gradient(f, *p, e);
            let l2 = dot(g, g).max(1e-8);
            let d = f(*p);
            // the gradient is a difference over 2e, so the step to the surface is d * 2e * g / |g|^2
            let k = d * 2.0 * e / l2;
            *p = [p[0] - g[0] * k, p[1] - g[1] * k, p[2] - g[2] * k];
        }
        let g = gradient(f, *p, e);
        let l = len(g).max(1e-6);
        normals.push([g[0] / l, g[1] / l, g[2] / l]);
    }
    (points, normals, idx)
}

fn gradient(f: &dyn Fn(V) -> f32, p: V, e: f32) -> V {
    [
        f([p[0] + e, p[1], p[2]]) - f([p[0] - e, p[1], p[2]]),
        f([p[0], p[1] + e, p[2]]) - f([p[0], p[1] - e, p[2]]),
        f([p[0], p[1], p[2] + e]) - f([p[0], p[1], p[2] - e]),
    ]
}

/// Where the skin is under a point just off it: a few steps down the field.
fn onto_skin(mut p: V) -> V {
    for _ in 0..3 {
        let g = gradient(&skin, p, 0.002);
        let l = len(g).max(1e-6);
        let d = skin(p).clamp(-0.08, 0.08);
        p = [p[0] - g[0] / l * d, p[1] - g[1] / l * d, p[2] - g[2] / l * d];
    }
    p
}

/// A skinned mesh of the shape `f` over the box `lo..hi`. Clothes (`cloth`) move as the skin under them does, so
/// that where the bones' pulls change quickly, under the arms say, the skin doesn't come through.
fn build(f: &dyn Fn(V) -> f32, lo: V, hi: V, h: f32, cloth: bool) -> Mesh {
    let (pts, nrm, idx) = surface_nets(f, lo, hi, h);
    let segs = segments();
    let mut verts = Vec::with_capacity(pts.len() * STRIDE);
    for (p, n) in pts.iter().zip(&nrm) {
        let (bi, bw) = weights(if cloth { onto_skin(*p) } else { *p }, &segs);
        // how far down an arm this is, in the arm's own frame, or 9 elsewhere
        let q = arm_down(*p);
        let on_arm = ((q[0] - ARM_X).powi(2) + q[2] * q[2]).sqrt() < 0.12 && q[1] < 1.47 && q[1] > 0.6 && p[0].abs() > 0.14;
        verts.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2], bi[0], bi[1], bi[2], bi[3], bw[0], bw[1], bw[2], bw[3], if on_arm { q[1] } else { 9.0 }]);
    }
    Mesh { verts, idx }
}

/// Every piece of the zombie's body, built once, and the living head the players wear instead of its own.
pub struct Figure {
    pub body: Mesh,
    pub head: Mesh,
    pub face: Mesh,
    pub hands: [Mesh; 2],
    pub shirt: Mesh,
    pub pants: Mesh,
    pub hair: Mesh,
    pub crop: Mesh,
}

impl Figure {
    /// `scale` makes every grid coarser (2 for far away, say), for less detail at a distance.
    pub fn new(scale: f32) -> Figure {
        let s = scale;
        let body = build(&body_part, [-0.62, -0.02, -0.28], [0.62, 1.52, 0.26], 0.011 * s, false);
        let head = build(&head_part, [-0.13, 1.48, -0.16], [0.13, 1.82, 0.17], 0.0045 * s, false);
        // the living face is meshed finer, for its lips and lids
        let face = build(&face_part, [-0.13, 1.48, -0.16], [0.13, 1.82, 0.17], 0.003 * s, false);
        let hands = [
            build(&hand_part, [-0.62, 0.66, -0.1], [-0.36, 0.98, 0.1], 0.0045 * s, false),
            build(&hand_part, [0.36, 0.66, -0.1], [0.62, 0.98, 0.1], 0.0045 * s, false),
        ];
        let shirt = build(&shirt, [-0.62, 0.82, -0.28], [0.62, 1.56, 0.26], 0.011 * s, true);
        let pants = build(&pants, [-0.3, -0.02, -0.3], [0.3, 1.06, 0.2], 0.011 * s, true);
        let hair = build(&hair, [-0.14, 1.48, -0.16], [0.14, 1.84, 0.18], 0.007 * s, false);
        let crop = build(&crop, [-0.14, 1.48, -0.16], [0.14, 1.84, 0.18], 0.006 * s, false);
        Figure { body, head, face, hands, shirt, pants, hair, crop }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_is_inside_the_body_and_outside_beyond_it() {
        assert!(skin([0.0, 1.2, 0.0]) < 0.0, "the middle of the chest");
        assert!(skin([0.0, 0.7, 0.0]) > 0.0, "between the legs, the thighs are 10 cm either side");
        assert!(skin([0.1, 0.7, 0.0]) < 0.0, "inside a thigh");
        assert!(skin([1.0, 1.0, 0.0]) > 0.0 && skin([0.0, 2.2, 0.0]) > 0.0);
        let (sn, cs) = ARM_ANGLE.sin_cos();
        let elbow = [ARM_X + sn * 0.28, SHOULDER_Y - cs * 0.28, 0.0];
        assert!(skin(elbow) < 0.0, "the arm sticks out at {elbow:?}");
    }

    #[test]
    fn weights_add_to_one_and_follow_the_nearest_bones() {
        let segs = segments();
        for p in [[0.0, 1.2, 0.0], [0.1, 0.4, 0.0], [-0.3, 1.2, 0.0], [0.0, 1.7, -0.09]] {
            let (_, w) = weights(p, &segs);
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        }
        let (i, w) = weights([0.098, 0.3, 0.0], &segs);
        assert_eq!(i[0] as usize, KNEE + 1, "a right shin follows the right knee bone, got {i:?} {w:?}");
        let (i, _) = weights([-0.098, 0.3, 0.0], &segs);
        assert_eq!(i[0] as usize, KNEE, "and a left one the left");
    }

    /// Ray-traces the rest pose from the front, the side, three quarters and the back into `$VIEWS` (a .ppm), to
    /// look at the shape without the game: `VIEWS=out.ppm cargo test --release views -- --ignored`. `$BARE` set
    /// leaves the clothes and hair off; `$FACE` set looks close at a living head instead.
    #[test]
    #[ignore]
    fn views() {
        let out = std::env::var("VIEWS").expect("set VIEWS to the picture to write");
        let bare = std::env::var("BARE").is_ok();
        let face = std::env::var("FACE").is_ok();
        let parts: Vec<(&(dyn Fn(V) -> f32 + Sync), [f32; 3])> = if face {
            let mut v: Vec<(&(dyn Fn(V) -> f32 + Sync), [f32; 3])> = vec![(&human_skin, [0.78, 0.62, 0.54])];
            if !bare {
                v.push((&crop, [0.15, 0.10, 0.07]));
            }
            v
        } else if bare {
            vec![(&skin, [0.78, 0.62, 0.54])]
        } else {
            vec![(&skin, [0.78, 0.62, 0.54]), (&shirt, [0.30, 0.42, 0.55]), (&pants, [0.25, 0.25, 0.30]), (&hair, [0.15, 0.10, 0.07])]
        };
        let scene = |p: V| parts.iter().map(|(f, _)| f(p)).fold(f32::MAX, f32::min);
        // the whole figure, or 32 cm round the head
        let (w, h, px, top) = if face { (400usize, 400usize, 0.32 / 400.0, 1.82) } else { (330usize, 640usize, 1.92 / 640.0, 1.9) };
        // looking from where, as a turn round the figure: front (it faces -z), side, three quarters, back
        let turns = if face { [std::f32::consts::PI, 2.6, std::f32::consts::FRAC_PI_2, 0.0] } else { [std::f32::consts::PI, std::f32::consts::FRAC_PI_2, 2.4, 0.0] };
        let mut img = vec![[0u8; 3]; w * h * turns.len()];
        std::thread::scope(|s| {
            for (v, chunk) in img.chunks_mut(w * h).enumerate() {
                let (scene, parts, a) = (&scene, &parts, turns[v]);
                s.spawn(move || {
                    let (sa, ca) = a.sin_cos();
                    // the camera looks along -dir; right is across the picture
                    let dir = [sa, 0.0, ca];
                    let right = [ca, 0.0, -sa];
                    for y in 0..h {
                        for x in 0..w {
                            let (u, t) = ((x as f32 - w as f32 / 2.0) * px, top - y as f32 * px);
                            let mut p = [right[0] * u + dir[0] * 1.0, t, right[2] * u + dir[2] * 1.0];
                            let mut col = [0.93, 0.93, 0.9];
                            if (t * 100.0).rem_euclid(if face { 1.0 } else { 10.0 }) < 0.3 * px / (1.92 / 640.0) {
                                col = [0.82, 0.82, 0.8];
                            }
                            for _ in 0..200 {
                                let d = scene(p);
                                if d < 0.0006 {
                                    let e = 0.001;
                                    let g = [
                                        scene([p[0] + e, p[1], p[2]]) - scene([p[0] - e, p[1], p[2]]),
                                        scene([p[0], p[1] + e, p[2]]) - scene([p[0], p[1] - e, p[2]]),
                                        scene([p[0], p[1], p[2] + e]) - scene([p[0], p[1], p[2] - e]),
                                    ];
                                    let l = len(g).max(1e-9);
                                    let n = [g[0] / l, g[1] / l, g[2] / l];
                                    let base = parts.iter().min_by(|a, b| a.0(p).partial_cmp(&b.0(p)).unwrap()).unwrap().1;
                                    let key = [0.5 * sa - 0.5 * ca, 0.6, 0.5 * ca + 0.5 * sa];
                                    let kl = len(key);
                                    let lit = 0.25 + 0.75 * (dot(n, key) / kl).max(0.0) + 0.15 * dot(n, dir).max(0.0);
                                    col = base.map(|c| c * lit);
                                    break;
                                }
                                p = [p[0] - dir[0] * d * 0.8, p[1], p[2] - dir[2] * d * 0.8];
                                if dot(p, dir) < -1.0 {
                                    break;
                                }
                            }
                            chunk[y * w + x] = col.map(|c| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8);
                        }
                    }
                });
            }
        });
        let mut ppm = format!("P6 {} {} 255\n", w * turns.len(), h).into_bytes();
        for y in 0..h {
            for v in 0..turns.len() {
                for x in 0..w {
                    ppm.extend_from_slice(&img[v * w * h + y * w + x]);
                }
            }
        }
        std::fs::write(out, ppm).unwrap();
    }

    #[test]
    fn the_figure_has_a_sensible_amount_of_surface() {
        let t = std::time::Instant::now();
        let f = Figure::new(2.0);
        println!("built in {:?}", t.elapsed());
        for (name, m) in [("body", &f.body), ("head", &f.head), ("face", &f.face), ("left hand", &f.hands[0]), ("right hand", &f.hands[1]), ("shirt", &f.shirt), ("pants", &f.pants), ("hair", &f.hair), ("crop", &f.crop)] {
            println!("{name}: {} vertices, {} triangles", m.verts.len() / STRIDE, m.idx.len() / 3);
            assert!(m.idx.len() > 300, "{name} is nearly empty");
            assert!(m.idx.iter().all(|i| (*i as usize) < m.verts.len() / STRIDE));
        }
    }
}
