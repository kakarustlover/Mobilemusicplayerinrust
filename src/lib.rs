//! Velora - a glass-style music player written entirely in Rust.
//! UI: egui/eframe - audio: rodio + symphonia - tags: lofty - Android glue: android-activity + jni.
//! The look is a 1:1 port of the HTML design (Music_Player_3.html).

use eframe::egui::{self, *};
use lofty::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;
#[cfg(target_os = "android")]
type Host = AndroidApp;
#[cfg(not(target_os = "android"))]
type Host = ();

const APP_NAME: &str = "Velora";

static F_REG: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static F_SEMI: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
static F_XBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-ExtraBold.ttf");

// ---------------------------------------------------------------- data

#[derive(Clone)]
struct Song {
    path: PathBuf,
    title: String,
    artist: String,
    album: String,
    dur: f32,
    rot: f32,
    cover: Option<std::sync::Arc<ColorImage>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Songs,
    Albums,
    Singers,
    Playlist,
}

#[derive(Clone, Copy, PartialEq)]
enum Pal {
    Pearl,
    Sky,
    Amber,
    Green,
}

enum Act {
    Play(usize),
    Filter(String),
}

// ---------------------------------------------------------------- fonts

fn f4(s: f32) -> FontId {
    FontId::new(s, FontFamily::Name("inter4".into()))
}
fn f6(s: f32) -> FontId {
    FontId::new(s, FontFamily::Name("inter6".into()))
}
fn f8(s: f32) -> FontId {
    FontId::new(s, FontFamily::Name("inter8".into()))
}

fn install_fonts(ctx: &Context) {
    let mut fd = FontDefinitions::default();
    fd.font_data.insert("inter4".into(), FontData::from_static(F_REG));
    fd.font_data.insert("inter6".into(), FontData::from_static(F_SEMI));
    fd.font_data.insert("inter8".into(), FontData::from_static(F_XBOLD));
    let fallback: Vec<String> = fd.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, key) in [("inter4", "inter4"), ("inter6", "inter6"), ("inter8", "inter8")] {
        let mut v = vec![key.to_string()];
        v.extend(fallback.iter().cloned());
        fd.families.insert(FontFamily::Name(name.into()), v);
    }
    let mut prop = vec!["inter4".to_string()];
    prop.extend(fallback);
    fd.families.insert(FontFamily::Proportional, prop);
    ctx.set_fonts(fd);
}

// ---------------------------------------------------------------- colors (CSS variables)

#[derive(Clone, Copy)]
struct C {
    dark: bool,
    bg1: Color32,
    bg2: Color32,
    tx: Color32,
    mu: Color32,
    a1: Color32,
    a2: Color32,
    b1: Color32,
    b2: Color32,
    b3: Color32,
    k1: Color32,
    k2: Color32,
    ac: Color32,
    acon: Color32,
    glow: Color32,
    sh: Color32,
    gb: Color32,
    card: Color32,
    sheet: Color32,
    track: Color32,
    bl: [Color32; 3],
    blo: [f32; 3],
    g1: f32,
    g2: f32,
    gl: f32,
}

fn hx(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

fn rgba(h: u32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied((h >> 16) as u8, (h >> 8) as u8, h as u8, (a.clamp(0., 1.) * 255.).round() as u8)
}

fn wa(a: f32) -> Color32 {
    rgba(0xffffff, a)
}

fn fade(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0., 1.))
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0., 1.);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    // premultiplied interpolation, exactly like CSS gradients
    Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}

fn sstep(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    x * x * (3. - 2. * x)
}

/// Colour set for one palette, in light or dark mode.
fn theme(dark: bool, pal: Pal) -> C {
    // (a1, a2, b1, b2, b3, k1, k2, ac, acon, glow, glow alpha, light bg1, light bg2, dark bg1, dark bg2)
    let (a1, a2, b1, b2, b3, k1, k2, ac, acon, glow, ga, l1, l2, d1, d2): (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, f32, u32, u32, u32, u32) = match pal {
        Pal::Pearl => (0xeceff3, 0x94a3b8, 0xd9d3c4, 0xe9e4d8, 0xf7f3ea, 0xf4f6fa, 0x98a5b8, 0x475569, 0x1e293b, 0x64748b, 0.36, 0xf1ebdf, 0xfbfaf6, 0x141416, 0x1e1e22),
        Pal::Sky => (0xbcd7fa, 0x5f8be0, 0xcfe2fb, 0xe0ecfc, 0xf2f7fe, 0x78a4f2, 0x4a73d6, 0x3f68d4, 0xffffff, 0x4a73d6, 0.32, 0xe8f0fa, 0xfbfdff, 0x0b1322, 0x14213a),
        Pal::Amber => (0xf8d3a6, 0xe58f45, 0xf7d9b4, 0xfae6cd, 0xfdf4e8, 0xf5b06a, 0xe0802f, 0xb4601c, 0x3b1d07, 0xe0802f, 0.32, 0xfbefe0, 0xfffaf3, 0x1a110a, 0x2a1b10),
        Pal::Green => (0xbfe3cd, 0x56a67c, 0xcdebd8, 0xe1f3e8, 0xf3fbf6, 0x6cc096, 0x3a8c63, 0x2f7d57, 0xffffff, 0x3a8c63, 0.32, 0xe7f3ea, 0xfafdfb, 0x0a1510, 0x12231b),
    };
    let (bg1, bg2) = if dark { (hx(d1), hx(d2)) } else { (hx(l1), hx(l2)) };
    let (a1c, a2c) = (hx(a1), hx(a2));
    let warm = pal == Pal::Pearl;
    let tx = match (dark, warm) {
        (false, false) => 0x0f172a,
        (false, true) => 0x1d1c1a,
        (true, false) => 0xf1f5f9,
        (true, true) => 0xf2f0ec,
    };
    let mu = match (dark, warm) {
        (false, false) => 0x475569,
        (false, true) => 0x625e56,
        (true, false) => 0xa3b3c8,
        (true, true) => 0xaaa69d,
    };
    let sheet = if dark {
        let m = mix(bg2, Color32::WHITE, 0.05);
        Color32::from_rgba_unmultiplied(m.r(), m.g(), m.b(), 244)
    } else {
        Color32::from_rgba_unmultiplied(bg2.r(), bg2.g(), bg2.b(), 242)
    };
    let bl = if dark { [mix(bg2, a2c, 0.5), mix(bg2, a2c, 0.38), mix(bg2, a1c, 0.28)] } else { [hx(b1), a2c, hx(b2)] };
    let blo = if dark { [0.7, 0.55, 0.45] } else { [0.6, 0.33, 0.6] };
    C {
        dark,
        bg1,
        bg2,
        tx: hx(tx),
        mu: hx(mu),
        a1: a1c,
        a2: a2c,
        b1: hx(b1),
        b2: hx(b2),
        b3: hx(b3),
        k1: hx(k1),
        k2: hx(k2),
        ac: hx(ac),
        acon: hx(acon),
        glow: rgba(glow, ga),
        sh: if dark { rgba(0x000000, 0.5) } else { rgba(0x0f172a, 0.12) },
        gb: wa(if dark { 0.22 } else { 0.9 }),
        card: wa(if dark { 0.08 } else { 0.62 }),
        sheet,
        track: if dark { wa(0.16) } else { rgba(0x0f172a, 0.12) },
        bl,
        blo,
        g1: if dark { 0.10 } else { 0.60 },
        g2: if dark { 0.03 } else { 0.20 },
        gl: if dark { 0.08 } else { 0.34 },
    }
}

// ---------------------------------------------------------------- math helpers

/// CSS cubic-bezier(x1, y1, x2, y2) evaluated at progress `u`.
fn bez(x1: f32, y1: f32, x2: f32, y2: f32, u: f32) -> f32 {
    let u = u.clamp(0., 1.);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let t = (lo + hi) / 2.;
        let x = 3. * (1. - t).powi(2) * t * x1 + 3. * (1. - t) * t * t * x2 + t.powi(3);
        if x < u {
            lo = t
        } else {
            hi = t
        }
    }
    let t = (lo + hi) / 2.;
    3. * (1. - t).powi(2) * t * y1 + 3. * (1. - t) * t * t * y2 + t.powi(3)
}
fn e_slide(u: f32) -> f32 {
    bez(0.2, 0.8, 0.2, 1., u)
}
fn e_ease(u: f32) -> f32 {
    bez(0.25, 0.1, 0.25, 1., u)
}
fn e_inout(u: f32) -> f32 {
    bez(0.42, 0., 0.58, 1., u)
}
fn e_out(u: f32) -> f32 {
    bez(0., 0., 0.58, 1., u)
}

/// Position of `p` along a CSS linear-gradient(<deg>) running over `rect` (0..1).
fn lin_t(rect: Rect, deg: f32, p: Pos2) -> f32 {
    let a = deg.to_radians();
    let d = vec2(a.sin(), -a.cos());
    let len = rect.width() * d.x.abs() + rect.height() * d.y.abs();
    (((p - rect.center()).dot(d)) / len.max(0.001) + 0.5).clamp(0., 1.)
}

fn fmt(s: f32) -> String {
    let s = s.max(0.) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

// ---------------------------------------------------------------- SVG icons (same path data as the HTML)

struct Sub {
    pts: Vec<Pos2>,
    closed: bool,
    corners: Vec<usize>,
}

struct Lex {
    s: Vec<char>,
    i: usize,
}

impl Lex {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_whitespace() || self.s[self.i] == ',') {
            self.i += 1;
        }
    }
    fn more_nums(&mut self) -> bool {
        self.skip();
        self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || matches!(self.s[self.i], '-' | '+' | '.'))
    }
    fn num(&mut self) -> f32 {
        self.skip();
        let st = self.i;
        if self.i < self.s.len() && matches!(self.s[self.i], '-' | '+') {
            self.i += 1;
        }
        let mut dot = false;
        while self.i < self.s.len() {
            let ch = self.s[self.i];
            if ch.is_ascii_digit() {
                self.i += 1;
            } else if ch == '.' && !dot {
                dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        self.s[st..self.i].iter().collect::<String>().parse::<f32>().unwrap_or(0.)
    }
    fn flag(&mut self) -> bool {
        self.skip();
        let v = self.i < self.s.len() && self.s[self.i] == '1';
        self.i += 1;
        v
    }
}

fn arc_pts(out: &mut Vec<Pos2>, p0: Pos2, rx: f32, ry: f32, phi_deg: f32, fa: bool, fs: bool, p1: Pos2) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-4 || ry < 1e-4 || (p0 - p1).length() < 1e-5 {
        out.push(p1);
        return;
    }
    let phi = phi_deg.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx = (p0.x - p1.x) / 2.;
    let dy = (p0.y - p1.y) / 2.;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1. {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.).sqrt();
    if fa == fs {
        co = -co;
    }
    let cx1 = co * rx * y1 / ry;
    let cy1 = -co * ry * x1 / rx;
    let cx = cp * cx1 - sp * cy1 + (p0.x + p1.x) / 2.;
    let cy = sp * cx1 + cp * cy1 + (p0.y + p1.y) / 2.;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let th1 = ang(1., 0., (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dth = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !fs && dth > 0. {
        dth -= TAU;
    }
    if fs && dth < 0. {
        dth += TAU;
    }
    let n = ((dth.abs() / (PI / 18.)).ceil() as usize).max(4);
    for k in 1..=n {
        let t = th1 + dth * k as f32 / n as f32;
        let (ct, st) = (t.cos(), t.sin());
        out.push(pos2(cp * rx * ct - sp * ry * st + cx, sp * rx * ct + cp * ry * st + cy));
    }
}

fn parse_path(d: &str) -> Vec<Sub> {
    let mut lx = Lex { s: d.chars().collect(), i: 0 };
    let mut subs: Vec<Sub> = Vec::new();
    let mut cur = pos2(0., 0.);
    let mut start = cur;
    let mut cmd = ' ';
    loop {
        lx.skip();
        if lx.i >= lx.s.len() {
            break;
        }
        let ch = lx.s[lx.i];
        if ch.is_ascii_alphabetic() {
            cmd = ch;
            lx.i += 1;
            if cmd == 'z' || cmd == 'Z' {
                if let Some(s) = subs.last_mut() {
                    s.closed = true;
                }
                cur = start;
                continue;
            }
        } else if !lx.more_nums() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { pos2(0., 0.) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let p = pos2(base.x + lx.num(), base.y + lx.num());
                subs.push(Sub { pts: vec![p], closed: false, corners: vec![0] });
                cur = p;
                start = p;
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' | 'H' | 'V' => {
                let p = match cmd.to_ascii_uppercase() {
                    'L' => pos2(base.x + lx.num(), base.y + lx.num()),
                    'H' => pos2(if rel { cur.x } else { 0. } + lx.num(), cur.y),
                    _ => pos2(cur.x, if rel { cur.y } else { 0. } + lx.num()),
                };
                if let Some(s) = subs.last_mut() {
                    s.pts.push(p);
                    s.corners.push(s.pts.len() - 1);
                }
                cur = p;
            }
            'C' => {
                let c1 = pos2(base.x + lx.num(), base.y + lx.num());
                let c2 = pos2(base.x + lx.num(), base.y + lx.num());
                let p = pos2(base.x + lx.num(), base.y + lx.num());
                if let Some(s) = subs.last_mut() {
                    for k in 1..=14 {
                        let t = k as f32 / 14.;
                        let u = 1. - t;
                        s.pts.push(pos2(
                            u * u * u * cur.x + 3. * u * u * t * c1.x + 3. * u * t * t * c2.x + t * t * t * p.x,
                            u * u * u * cur.y + 3. * u * u * t * c1.y + 3. * u * t * t * c2.y + t * t * t * p.y,
                        ));
                    }
                    s.corners.push(s.pts.len() - 1);
                }
                cur = p;
            }
            'A' => {
                let rx = lx.num();
                let ry = lx.num();
                let rot = lx.num();
                let fa = lx.flag();
                let fs = lx.flag();
                let p = pos2(base.x + lx.num(), base.y + lx.num());
                if let Some(s) = subs.last_mut() {
                    arc_pts(&mut s.pts, cur, rx, ry, rot, fa, fs, p);
                    s.corners.push(s.pts.len() - 1);
                }
                cur = p;
            }
            _ => break,
        }
    }
    subs
}

type Dots = &'static [(f32, f32, f32)];
const NO_DOTS: Dots = &[];
const PAL_DOTS: Dots = &[(8., 11., 1.), (12., 7.5, 1.), (16., 11., 1.)];
const SEARCH_DOTS: Dots = &[(11., 11., 7.)];
const SUN_DOTS: Dots = &[(12., 12., 4.)];

/// (stroke path, circles (cx, cy, r), filled?)
fn icon_def(k: &str) -> (&'static str, Dots, bool) {
    match k {
        "pal" => ("M12 3a9 9 0 1 0 0 18c1.4 0 2-1 1.5-2-.6-1.2.2-2.5 1.6-2.5H17a4 4 0 0 0 4-4C21 6.6 17 3 12 3z", PAL_DOTS, false),
        "search" => ("m20 20-3.5-3.5", SEARCH_DOTS, false),
        "sun" => ("M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5 19 19M5 19l1.5-1.5M17.5 6.5 19 5", SUN_DOTS, false),
        "moon" => ("M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z", NO_DOTS, false),
        "shuf" => ("M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5", NO_DOTS, false),
        "rep" => ("M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3", NO_DOTS, false),
        "x" => ("M6 6l12 12M18 6 6 18", NO_DOTS, false),
        "down" => ("M6 9l6 6 6-6", NO_DOTS, false),
        "vol" => ("M4 9v6h4l5 4V5L8 9zM16.5 9a4 4 0 0 1 0 6", NO_DOTS, false),
        "play" => ("M8 5v14l11-7z", NO_DOTS, true),
        "pause" => ("M6 5h4v14H6zM14 5h4v14h-4z", NO_DOTS, true),
        "prev" => ("M6 6h2v12H6zM9.5 12 18 18V6z", NO_DOTS, true),
        "next" => ("M16 6h2v12h-2zM6 18l8.5-6L6 6z", NO_DOTS, true),
        _ => ("", NO_DOTS, false),
    }
}

fn icon(p: &Painter, k: &str, ce: Pos2, size: f32, col: Color32) {
    let (d, circles, filled) = icon_def(k);
    let u = size / 24.;
    let o = ce - vec2(12., 12.) * u;
    let map = |q: Pos2| pos2(o.x + q.x * u, o.y + q.y * u);
    let sw = 1.8 * u;
    let st = Stroke::new(sw, col);
    for s in parse_path(d) {
        let pts: Vec<Pos2> = s.pts.iter().map(|&q| map(q)).collect();
        if filled {
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        } else {
            if s.closed {
                p.add(Shape::closed_line(pts.clone(), st));
            } else {
                p.add(Shape::line(pts.clone(), st));
            }
            // round caps / joins
            for &ci in &s.corners {
                if let Some(q) = pts.get(ci) {
                    p.circle_filled(*q, sw / 2., col);
                }
            }
            if !s.closed {
                if let Some(q) = pts.last() {
                    p.circle_filled(*q, sw / 2., col);
                }
            }
        }
    }
    for &(cx, cy, r) in circles {
        let c = map(pos2(cx, cy));
        if r * u <= sw / 2. + 0.05 {
            p.circle_filled(c, r * u + sw / 2., col);
        } else {
            p.circle_stroke(c, r * u, st);
        }
    }
}

// ---------------------------------------------------------------- graphics primitives

fn sdf_cov(px: f32, py: f32, w: f32, h: f32, r: f32, texel: f32) -> f32 {
    let qx = (px - w / 2.).abs() - (w / 2. - r);
    let qy = (py - h / 2.).abs() - (h / 2. - r);
    let d = (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt() + qx.max(qy).min(0.) - r;
    (0.5 - d / texel).clamp(0., 1.)
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    OuterCut,
    Plain,
    Inner,
}

struct TexInfo {
    id: TextureId,
    off: Vec2,
    size: Vec2,
}

#[derive(Default)]
struct Fx {
    tex: HashMap<String, (TextureHandle, Vec2, Vec2)>,
}

impl Fx {
    /// Blurred rounded-rect mask texture (white, premultiplied alpha).
    fn blur_tex(&mut self, ctx: &Context, w: f32, h: f32, r: f32, off: Vec2, sigma: f32, mode: Mode) -> TexInfo {
        let r = r.min(w / 2.).min(h / 2.).max(0.);
        let key = format!("{:.1}|{:.1}|{:.1}|{:.1}|{:.1}|{:.1}|{}", w, h, r, off.x, off.y, sigma, mode as u8);
        if let Some((th, o, s)) = self.tex.get(&key) {
            return TexInfo { id: th.id(), off: *o, size: *s };
        }
        let texel = if sigma < 5. { 0.5 } else if sigma < 12. { 1.0 } else if sigma < 25. { 2.0 } else { 4.0 };
        let m = (3. * sigma).ceil() + off.x.abs().max(off.y.abs()).ceil();
        let pw = ((w + 2. * m) / texel).ceil() as usize;
        let ph = ((h + 2. * m) / texel).ceil() as usize;
        let pos = |ix: usize, iy: usize| ((ix as f32 + 0.5) * texel - m, (iy as f32 + 0.5) * texel - m);
        let mut a = vec![0f32; pw * ph];
        for iy in 0..ph {
            for ix in 0..pw {
                let (x, y) = pos(ix, iy);
                a[iy * pw + ix] = sdf_cov(x - off.x, y - off.y, w, h, r, texel);
            }
        }
        let st = (sigma / texel).max(0.01);
        let kr = (3. * st).ceil() as i32;
        let mut ker: Vec<f32> = (-kr..=kr).map(|i| (-(i * i) as f32 / (2. * st * st)).exp()).collect();
        let sum: f32 = ker.iter().sum();
        ker.iter_mut().for_each(|k| *k /= sum);
        let mut b = vec![0f32; pw * ph];
        for iy in 0..ph {
            for ix in 0..pw as i32 {
                let mut acc = 0.;
                for (ki, kv) in ker.iter().enumerate() {
                    let x = ix + ki as i32 - kr;
                    if x >= 0 && x < pw as i32 {
                        acc += kv * a[iy * pw + x as usize];
                    }
                }
                b[iy * pw + ix as usize] = acc;
            }
        }
        for iy in 0..ph as i32 {
            for ix in 0..pw {
                let mut acc = 0.;
                for (ki, kv) in ker.iter().enumerate() {
                    let y = iy + ki as i32 - kr;
                    if y >= 0 && y < ph as i32 {
                        acc += kv * b[y as usize * pw + ix];
                    }
                }
                a[iy as usize * pw + ix] = acc;
            }
        }
        let mut pixels = Vec::with_capacity(pw * ph);
        for iy in 0..ph {
            for ix in 0..pw {
                let (x, y) = pos(ix, iy);
                let bl = a[iy * pw + ix];
                let v = match mode {
                    Mode::Plain => bl,
                    Mode::OuterCut => bl * (1. - sdf_cov(x, y, w, h, r, texel)),
                    Mode::Inner => sdf_cov(x, y, w, h, r, texel) * (1. - bl),
                };
                let v8 = (v.clamp(0., 1.) * 255.).round() as u8;
                pixels.push(Color32::from_rgba_premultiplied(v8, v8, v8, v8));
            }
        }
        let img = ColorImage { size: [pw, ph], pixels };
        let th = ctx.load_texture(format!("fx{}", self.tex.len()), img, TextureOptions::LINEAR);
        let o = vec2(-m, -m);
        let s = vec2(pw as f32 * texel, ph as f32 * texel);
        let id = th.id();
        self.tex.insert(key, (th, o, s));
        TexInfo { id, off: o, size: s }
    }
}

struct Gfx {
    p: Painter,
    ctx: Context,
    fx: Fx,
    c: C,
    /// (centre, radius, colour, strength) of everything glassy/coloured behind the UI
    tint: Vec<(Pos2, f32, Color32, f32)>,
}

fn tex_quad(p: &Painter, id: TextureId, rect: Rect, tint: Color32) {
    let mut m = Mesh::with_texture(id);
    m.add_rect_with_uv(rect, Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)), tint);
    p.add(Shape::mesh(m));
}

/// Textured rounded rect (cover art). `rot` rotates the picture around the centre.
fn tex_round(p: &Painter, id: TextureId, rect: Rect, r: f32, rot: f32, tint: Color32) {
    let pts = outline(rect, r);
    let c = rect.center();
    let (sn, cs) = rot.sin_cos();
    let uv = |q: Pos2| {
        let d = q - c;
        pos2(0.5 + (d.x * cs + d.y * sn) / rect.width(), 0.5 + (-d.x * sn + d.y * cs) / rect.height())
    };
    let mut m = Mesh::with_texture(id);
    m.vertices.push(egui::epaint::Vertex { pos: c, uv: uv(c), color: tint });
    for (q, _) in &pts {
        m.vertices.push(egui::epaint::Vertex { pos: *q, uv: uv(*q), color: tint });
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

/// Rounded-rect with a per-vertex colour function. `kinks` are y fractions where the function is only piecewise-linear.
fn fill(p: &Painter, rect: Rect, r: f32, kinks: &[f32], f: &dyn Fn(Pos2) -> Color32) {
    fill_band(p, rect, r, 0., rect.height(), kinks, f);
}

/// Like `fill`, but only paints the horizontal band `y0..y1` (pixels from the top) of the rounded shape.
fn fill_band(p: &Painter, rect: Rect, r: f32, y0: f32, y1: f32, kinks: &[f32], f: &dyn Fn(Pos2) -> Color32) {
    fill_band_x(p, rect, r, y0, y1, kinks, 1, f);
}

/// Same as `fill_band` but also cut into `nx` columns so smooth 2-D colour fields (tints, ripples) look right.
fn fill_band_x(p: &Painter, rect: Rect, r: f32, y0: f32, y1: f32, kinks: &[f32], nx: usize, f: &dyn Fn(Pos2) -> Color32) {
    let nx = nx.max(1);
    let (w, h) = (rect.width(), rect.height());
    let r = r.min(w / 2.).min(h / 2.).max(0.);
    let mut ys = vec![0., h];
    if r > 0.5 {
        for k in 1..=12 {
            let a = k as f32 / 12. * FRAC_PI_2;
            let dy = r * (1. - a.cos());
            ys.push(dy);
            ys.push(h - dy);
        }
        ys.push(r);
        ys.push(h - r);
    }
    for k in kinks {
        ys.push(k * h);
    }
    ys.push(y0);
    ys.push(y1);
    ys.retain(|v| *v >= y0 - 0.001 && *v <= y1 + 0.001);
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
    let inset = |dy: f32| -> f32 {
        let d = if dy < r {
            r - dy
        } else if dy > h - r {
            dy - (h - r)
        } else {
            return 0.;
        };
        r - (r * r - d * d).max(0.).sqrt()
    };
    let mut m = Mesh::default();
    for win in ys.windows(2) {
        let (y0, y1) = (win[0], win[1]);
        let (i0, i1) = (inset(y0), inset(y1));
        let n = m.vertices.len() as u32;
        for j in 0..=nx {
            let t = j as f32 / nx as f32;
            let qa = pos2(rect.left() + i0 + (w - 2. * i0) * t, rect.top() + y0);
            m.colored_vertex(qa, f(qa));
            let qb = pos2(rect.left() + i1 + (w - 2. * i1) * t, rect.top() + y1);
            m.colored_vertex(qb, f(qb));
        }
        for j in 0..nx as u32 {
            let a = n + 2 * j;
            m.add_triangle(a, a + 1, a + 3);
            m.add_triangle(a, a + 3, a + 2);
        }
    }
    p.add(Shape::mesh(m));
}

fn add_col(a: Color32, b: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(a.r().saturating_add(b.r()), a.g().saturating_add(b.g()), a.b().saturating_add(b.b()), a.a().saturating_add(b.a()))
}

/// Colour that glass picks up from the blobs / orbs behind it at point `q`.
fn tint_at(t: &[(Pos2, f32, Color32, f32)], q: Pos2, a: f32) -> Color32 {
    let mut acc = [0f32; 4];
    for (ce, rad, col, op) in t {
        let d = (q - *ce).length() / (rad * 0.85);
        let w = (-d * d).exp() * op * a;
        acc[0] += col.r() as f32 * w;
        acc[1] += col.g() as f32 * w;
        acc[2] += col.b() as f32 * w;
        acc[3] += col.a() as f32 * w;
    }
    let u = |v: f32| v.clamp(0., 255.) as u8;
    Color32::from_rgba_premultiplied(u(acc[0]), u(acc[1]), u(acc[2]), u(acc[3]))
}

/// iOS-style rim light: a thin ring that is bright where the light hits (top-left, softer bottom-right).
fn rim(p: &Painter, rect: Rect, r: f32, th: f32, a: f32, dark: bool) {
    let pts = outline(rect, r);
    let n = pts.len();
    let k = if dark { 0.75 } else { 1.0 };
    let mut m = Mesh::default();
    for (q, nr) in &pts {
        let l1 = (nr.x * -0.55 + nr.y * -0.83).max(0.);
        let l2 = (nr.x * 0.55 + nr.y * 0.83).max(0.);
        let al = ((0.30 + 0.70 * l1.powf(1.4) + 0.40 * l2.powf(2.)) * k * a).clamp(0., 1.);
        m.colored_vertex(*q, wa(al));
        m.colored_vertex(*q - *nr * th, wa(al * 0.25));
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (a0, b0, c0, d0) = (2 * i as u32, 2 * i as u32 + 1, 2 * j as u32, 2 * j as u32 + 1);
        m.add_triangle(a0, b0, c0);
        m.add_triangle(b0, d0, c0);
    }
    p.add(Shape::mesh(m));
}

/// Outline points of a rounded rect with outward normals (clockwise from top-left arc end).
fn outline(rect: Rect, r: f32) -> Vec<(Pos2, Vec2)> {
    let r = r.min(rect.width() / 2.).min(rect.height() / 2.).max(0.);
    let corners = [
        (rect.right() - r, rect.top() + r, -90.0f32),
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, 90.0),
        (rect.left() + r, rect.top() + r, 180.0),
    ];
    let mut v = Vec::new();
    for (cx, cy, a0) in corners {
        for k in 0..=10 {
            let a = (a0 + 90.0 * k as f32 / 10.0).to_radians();
            let n = vec2(a.cos(), a.sin());
            v.push((pos2(cx, cy) + n * r, n));
        }
    }
    v
}

/// `inset 0 <off>px 0 <colour>`: a hard crescent hugging the top edge.
fn crescent(p: &Painter, rect: Rect, r: f32, off: f32, col: Color32) {
    // inset shadows live inside the 1px border
    let (rect, r) = (rect.shrink(1.), (r - 1.).max(0.));
    let pts = outline(rect, r);
    let mut m = Mesh::default();
    let n = pts.len();
    for i in 0..n {
        let (q, nr) = pts[i];
        let th = off * (-nr.y).max(0.);
        m.colored_vertex(q, col);
        m.colored_vertex(q - nr * th, col);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let (ti, tj) = (off * (-pts[i].1.y).max(0.), off * (-pts[j].1.y).max(0.));
        if ti <= 0. && tj <= 0. {
            continue;
        }
        let (a, b, c, d) = (2 * i as u32, 2 * i as u32 + 1, 2 * j as u32, 2 * j as u32 + 1);
        m.add_triangle(a, b, c);
        m.add_triangle(b, d, c);
    }
    p.add(Shape::mesh(m));
}

fn ray_rrect(rect: Rect, r: f32, ang: f32) -> Pos2 {
    let c = rect.center();
    let d = vec2(ang.sin(), -ang.cos());
    let (hw, hh) = (rect.width() / 2., rect.height() / 2.);
    let r = r.min(hw).min(hh);
    let sdf = |t: f32| {
        let q = d * t;
        let qx = q.x.abs() - (hw - r);
        let qy = q.y.abs() - (hh - r);
        (qx.max(0.).powi(2) + qy.max(0.).powi(2)).sqrt() + qx.max(qy).min(0.) - r
    };
    let (mut lo, mut hi) = (0., (hw * hw + hh * hh).sqrt() + 2.);
    for _ in 0..18 {
        let mid = (lo + hi) / 2.;
        if sdf(mid) < 0. {
            lo = mid
        } else {
            hi = mid
        }
    }
    c + d * ((lo + hi) / 2.)
}

/// CSS conic-gradient(from <from_deg>, a, b, c, a) clipped to a rounded rect.
fn conic(p: &Painter, rect: Rect, r: f32, from_deg: f32, st: [Color32; 4], wedges: usize) {
    let col = |f: f32| {
        let s = f.clamp(0., 1.) * 3.;
        let i = (s as usize).min(2);
        mix(st[i], st[i + 1], s - i as f32)
    };
    let c = rect.center();
    let mut m = Mesh::default();
    for k in 0..wedges {
        let f0 = k as f32 / wedges as f32;
        let f1 = (k + 1) as f32 / wedges as f32;
        let a0 = (from_deg + f0 * 360.).to_radians();
        let a1 = (from_deg + f1 * 360.).to_radians();
        let n = m.vertices.len() as u32;
        m.colored_vertex(c, col((f0 + f1) / 2.));
        m.colored_vertex(ray_rrect(rect, r, a0), col(f0));
        m.colored_vertex(ray_rrect(rect, r, a1), col(f1));
        m.add_triangle(n, n + 1, n + 2);
    }
    p.add(Shape::mesh(m));
}

fn ellipsize(p: &Painter, s: &str, font: FontId, max_w: f32) -> String {
    let w = |t: &str| p.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x;
    if w(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let t: String = chars[..mid].iter().collect::<String>() + "\u{2026}";
        if w(&t) <= max_w {
            lo = mid
        } else {
            hi = mid - 1
        }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_string() + "\u{2026}"
}

impl Gfx {
    fn shadow(&mut self, rect: Rect, r: f32, off: Vec2, blur: f32, col: Color32) {
        let t = self.fx.blur_tex(&self.ctx, rect.width(), rect.height(), r, off, blur / 2., Mode::OuterCut);
        tex_quad(&self.p, t.id, Rect::from_min_size(rect.min + t.off, t.size), col);
    }

    fn inner_shadow(&mut self, rect: Rect, r: f32, off: Vec2, blur: f32, col: Color32) {
        let t = self.fx.blur_tex(&self.ctx, rect.width(), rect.height(), r, off, blur / 2., Mode::Inner);
        tex_quad(&self.p, t.id, Rect::from_min_size(rect.min + t.off, t.size), col);
    }

    fn border(&self, rect: Rect, r: f32, col: Color32) {
        self.p.rect_stroke(rect.shrink(0.5), (r - 0.5).max(0.), Stroke::new(1., col));
    }

    /// The `::after` gloss lens of `.glass`: the top 48% of the padding box, clipped to the button's own
    /// rounded shape so it can never poke out at the corners. `ext_top` extends the shape upward (top sheet).
    fn lens(&self, rect: Rect, r: f32, strength: f32, a: f32, ext_top: f32) {
        let inner = rect.shrink(1.);
        let ri = (r - 1.).max(0.);
        let hh = inner.height() * 0.48;
        let full = Rect::from_min_max(pos2(inner.left(), inner.top() - ext_top), inner.right_bottom());
        let top = inner.top();
        fill_band(&self.p, full, ri, ext_top, ext_top + hh, &[], &|q| wa(0.7 * (1. - (q.y - top) / hh).clamp(0., 1.) * strength * a));
    }

    /// Light ripple that blooms from the finger inside a pressed button (clipped to the button's own shape).
    fn pressfx(&self, rect: Rect, r: f32, s: f32) {
        let k = ((1. - s) / 0.08).clamp(0., 1.);
        if k < 0.02 {
            return;
        }
        let c = self.c;
        let pp = self.ctx.input(|i| i.pointer.interact_pos()).unwrap_or(rect.center());
        let pos = pos2(pp.x.clamp(rect.left(), rect.right()), pp.y.clamp(rect.top(), rect.bottom()));
        let rad = rect.width().min(rect.height()) * 1.15 + 6.;
        let col = if c.dark { wa(0.34) } else { fade(c.k1, 0.38) };
        let nx = ((rect.width() / 7.).ceil() as usize).clamp(2, 28);
        fill_band_x(&self.p, rect, r, 0., rect.height(), &[], nx, &|q| {
            let t = (1. - (q - pos).length() / rad).clamp(0., 1.);
            fade(col, t * t * k)
        });
    }

    /// A glass bubble floating in the background (buttons that pass over it pick up its light).
    fn orb(&mut self, ce: Pos2, d: f32) {
        let c = self.c;
        let rect = Rect::from_center_size(ce, vec2(d, d));
        let (hi, lo) = if c.dark { (0.11, 0.025) } else { (0.62, 0.16) };
        self.shadow(rect, d / 2., vec2(0., 10.), 30., fade(c.sh, 0.7));
        fill(&self.p, rect, d / 2., &[], &|q| wa(hi + (lo - hi) * lin_t(rect, 150., q)));
        self.inner_shadow(rect, d / 2., vec2(0., -10.), 18., wa(0.10));
        self.lens(rect, d / 2., c.gl, 1., 0.);
        rim(&self.p, rect.shrink(0.5), d / 2. - 0.5, 1.6, 1., c.dark);
        self.tint.push((ce, d / 2., Color32::WHITE, if c.dark { 0.25 } else { 0.5 }));
    }

    /// Press feedback: 1.0 normally, eases to 0.92 while the pointer holds the button down.
    fn press(&self, key: &str, rect: Rect, on: bool) -> f32 {
        let down = on && self.ctx.input(|i| i.pointer.primary_down() && i.pointer.interact_pos().map_or(false, |p| rect.contains(p)));
        self.ctx.animate_value_with_time(Id::new(("press", key)), if down { 0.92 } else { 1.0 }, 0.12)
    }

    /// `.glass`: slim liquid-glass: light translucent body that picks up colour from what is behind it,
    /// soft low shadow, inner glow, rim light.
    fn glass(&mut self, rect: Rect, r: f32, a: f32) {
        let c = self.c;
        self.shadow(rect, r, vec2(0., 6.), 22., fade(c.sh, a * 0.8));
        let (g1, g2) = (c.g1, c.g2);
        let tint = self.tint.clone();
        let nx = ((rect.width() / 18.).ceil() as usize).clamp(1, 24);
        fill_band_x(&self.p, rect, r, 0., rect.height(), &[], nx, &|q| add_col(wa((g1 + (g2 - g1) * lin_t(rect, 150., q)) * a), tint_at(&tint, q, a)));
        self.inner_shadow(rect, r, vec2(0., -8.), 14., wa(0.07 * a));
        self.p.rect_stroke(rect.shrink(0.4), (r - 0.4).max(0.), Stroke::new(0.8, fade(rgba(0x0f172a, if c.dark { 0.0 } else { 0.07 }), a)));
        self.lens(rect, r, c.gl, a, 0.);
        rim(&self.p, rect.shrink(0.5), r - 0.5, 1.1, a, c.dark);
    }

    /// Opaque-ish sheet used by the mini player and the appearance panel (`background: var(--sheet)` + glass chrome).
    /// `lens_rect` is the logical box; when it differs from `rect` the sheet was extended upward past the screen edge.
    fn sheet(&mut self, rect: Rect, r: f32, lens_rect: Rect) {
        let c = self.c;
        self.shadow(rect, r, vec2(0., 10.), 30., c.sh);
        fill(&self.p, rect, r, &[], &|_| c.sheet);
        self.inner_shadow(rect, r, vec2(0., -10.), 18., wa(0.08));
        self.border(rect, r, c.gb);
        let ext_top = lens_rect.top() - rect.top();
        if ext_top > 0.5 {
            self.p.rect_filled(Rect::from_min_size(lens_rect.min, vec2(lens_rect.width(), 1.)), 0., c.gb);
            self.p.rect_filled(Rect::from_min_size(lens_rect.min + vec2(1., 1.), vec2(lens_rect.width() - 2., 1.5)), 0., wa(0.75));
        } else {
            crescent(&self.p, rect, r, 1.5, wa(0.75));
        }
        self.lens(lens_rect, r, c.gl, 1., ext_top.max(0.));
    }

    /// `--ag` accent gradient with its hard gloss step.
    fn ag_fill(&self, rect: Rect, r: f32) {
        let c = self.c;
        let top = rect.top();
        let h = rect.height();
        fill(&self.p, rect, r, &[0.52, 0.53], &|q| {
            let base = mix(c.k1, c.k2, lin_t(rect, 145., q));
            let y = (q.y - top) / h;
            let al = if y < 0.52 { 0.5 + (0.06 - 0.5) * (y / 0.52) } else if y < 0.53 { 0.06 * (1. - (y - 0.52) / 0.01) } else { 0. };
            mix(base, Color32::WHITE, al)
        });
    }

    /// Accent button (`.ib.act`, `.mb .ib.go`, selected segment): glow, gradient, border, highlight.
    fn accent(&mut self, rect: Rect, r: f32, glass_lens: bool) {
        let c = self.c;
        self.shadow(rect, r, vec2(0., 8.), 20., c.glow);
        self.ag_fill(rect, r);
        rim(&self.p, rect.shrink(0.5), r - 0.5, 1.1, 1., false);
        if glass_lens {
            self.lens(rect, r, c.gl, 1., 0.);
        }
    }

    /// The big round play button.
    fn play_btn(&mut self, rect: Rect) {
        let c = self.c;
        let r = rect.width() / 2.;
        self.shadow(rect, r, vec2(0., 16.), 36., c.glow);
        self.ag_fill(rect, r);
        self.inner_shadow(rect, r, vec2(0., -12.), 20., wa(0.12));
        rim(&self.p, rect.shrink(0.5), r - 0.5, 1.4, 1., false);
    }

    fn bg_linear(&self, rect: Rect, c1: Color32, c2: Color32, deg: f32) {
        let mut m = Mesh::default();
        for q in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()] {
            m.colored_vertex(q, mix(c1, c2, lin_t(rect, deg, q)));
        }
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        self.p.add(Shape::mesh(m));
    }

    /// radial-gradient(<rx>% <ry>% at <cx>% <cy>%, col, transparent <stop>)
    fn radial(&self, rect: Rect, cx: f32, cy: f32, rx: f32, ry: f32, col: Color32, stop: f32, a: f32) {
        let ce = pos2(rect.left() + cx * rect.width(), rect.top() + cy * rect.height());
        let (rxp, ryp) = (rx * rect.width() * stop, ry * rect.height() * stop);
        let mut m = Mesh::default();
        m.colored_vertex(ce, fade(col, a));
        let n = 96;
        for k in 0..=n {
            let t = k as f32 / n as f32 * TAU;
            m.colored_vertex(ce + vec2(t.cos() * rxp, t.sin() * ryp), Color32::TRANSPARENT);
        }
        for k in 0..n as u32 {
            m.add_triangle(0, 1 + k, 2 + k);
        }
        self.p.add(Shape::mesh(m));
    }

    fn blobs(&mut self, sr: Rect, now: f64, a: f32) {
        let c = self.c;
        let (w, h) = (sr.width(), sr.height());
        let defs = [
            (300., pos2(sr.left() + w - 60., sr.top() + 80.), c.bl[0], c.blo[0], 0.),
            (260., pos2(sr.left() + 30., sr.top() + h - 220.), c.bl[1], c.blo[1], 5.),
            (220., pos2(sr.left() + w - 40., sr.top() + 0.42 * h + 110.), c.bl[2], c.blo[2], 9.),
        ];
        for (d, ce, col, op, delay) in defs {
            let t = (now + delay) / 14.;
            let fr = (t % 2.) as f32;
            let u = if fr < 1. { fr } else { 2. - fr };
            let e = e_inout(u);
            let ce = ce + vec2(30. * e, -40. * e);
            let sc = 1. + 0.15 * e;
            let t = self.fx.blur_tex(&self.ctx, d, d, d / 2., vec2(0., 0.), 60., Mode::Plain);
            let top_left = ce - vec2(d, d) * sc / 2.;
            let rect = Rect::from_min_size(top_left + t.off * sc, t.size * sc);
            tex_quad(&self.p, t.id, rect, fade(col, op * a));
            self.tint.push((ce, d * sc / 2., col, op * if c.dark { 1.1 } else { 0.8 }));
        }
    }

    fn home_bg(&mut self, sr: Rect, now: f64) {
        let c = self.c;
        self.bg_linear(sr, c.bg1, c.bg2, 160.);
        self.blobs(sr, now, 1.);
        let (w, h) = (sr.width(), sr.height());
        let t = now as f32;
        self.orb(pos2(sr.left() + w * 0.80 + 12. * (t * 0.35).sin(), sr.top() + 190. + 16. * (t * 0.28).cos()), 118.);
        self.orb(pos2(sr.left() + w * 0.13 + 10. * (t * 0.30 + 1.).cos(), sr.top() + h * 0.60 + 14. * (t * 0.33).sin()), 84.);
    }

    /// Spinning record: glow, conic body, groove rings, centre hole.
    fn disc(&mut self, ce: Pos2, d: f32, rot_deg: f32, a: f32) {
        let c = self.c;
        let rect = Rect::from_center_size(ce, vec2(d, d));
        self.shadow_plain(rect, d / 2., vec2(0., 14.), 34., fade(c.glow, a));
        conic(&self.p, rect, d / 2., rot_deg, [fade(c.a1, a), fade(c.a2, a), fade(c.b3, a), fade(c.a1, a)], 128);
        let mut rr = 7.5;
        while rr + 0.5 <= d / 2. {
            self.p.circle_stroke(ce, rr, Stroke::new(1., wa(0.16 * a)));
            rr += 8.;
        }
        let hole = Rect::from_center_size(ce, vec2(d * 0.28, d * 0.28));
        self.p.circle_filled(ce, d * 0.14, fade(c.bg2, a));
        self.inner_shadow(hole, d * 0.14, vec2(0., 2.), 6., fade(c.sh, a));
        self.p.circle_stroke(ce, d * 0.14 - 1.5, Stroke::new(3., wa(0.8 * a)));
    }

    /// Soft glow that is *not* cut out under the element (the disc is opaque anyway).
    fn shadow_plain(&mut self, rect: Rect, r: f32, off: Vec2, blur: f32, col: Color32) {
        let t = self.fx.blur_tex(&self.ctx, rect.width(), rect.height(), r, off, blur / 2., Mode::OuterCut);
        tex_quad(&self.p, t.id, Rect::from_min_size(rect.min + t.off, t.size), col);
    }

    /// `<input type=range class=rg>`: 36px tall hit area, 6px track, 20px thumb.
    fn range(&mut self, area: Rect, f: f32) {
        let c = self.c;
        let cy = area.center().y;
        let track = Rect::from_min_max(pos2(area.left(), cy - 3.), pos2(area.right(), cy + 3.));
        self.p.rect_filled(track, 3., c.track);
        let fw = f.clamp(0., 1.) * track.width();
        if fw > 0.5 {
            let clip = Rect::from_min_max(track.min, pos2(track.left() + fw, track.bottom()));
            let pc = self.p.with_clip_rect(clip.intersect(self.p.clip_rect()));
            let (a1, a2, l) = (c.a1, c.a2, track.left());
            fill(&pc, track, 3., &[], &|q| mix(a1, a2, (q.x - l) / fw));
        }
        let tp = pos2(area.left() + 10. + f.clamp(0., 1.) * (area.width() - 20.), cy);
        let tr = Rect::from_center_size(tp, vec2(20., 20.));
        self.shadow(tr, 10., vec2(0., 3.), 10., c.sh);
        self.p.circle_filled(tp, 10., Color32::WHITE);
        self.p.circle_stroke(tp, 9., Stroke::new(2., c.ac));
    }
}

fn txt(p: &Painter, pos: Pos2, al: Align2, s: impl ToString, font: FontId, col: Color32) -> Rect {
    p.text(pos, al, s.to_string(), font, col)
}

// ---------------------------------------------------------------- library scan (tags via lofty)

/// Decodes an embedded cover picture into a small square image.
fn decode_cover(bytes: &[u8]) -> Option<std::sync::Arc<ColorImage>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.resize_to_fill(160, 160, image::imageops::FilterType::Triangle).to_rgba8();
    let (w, h) = img.dimensions();
    Some(std::sync::Arc::new(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw())))
}

fn read_meta(p: &Path) -> Song {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let (mut t, mut a, mut al, mut d) = (stem, "Unknown".to_string(), "Unknown".to_string(), 0.0f32);
    let mut cover = None;
    if let Ok(tf) = lofty::read_from_path(p) {
        d = tf.properties().duration().as_secs_f32();
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            if let Some(x) = tag.title() {
                t = x.to_string();
            }
            if let Some(x) = tag.artist() {
                a = x.to_string();
            }
            if let Some(x) = tag.album() {
                al = x.to_string();
            }
            if let Some(pic) = tag.pictures().first() {
                cover = decode_cover(pic.data());
            }
        }
    }
    let rot = (t.bytes().map(|b| b as u32).sum::<u32>() % 360) as f32;
    Song { path: p.to_path_buf(), title: t, artist: a, album: al, dur: d, rot, cover }
}

fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            if name != "Android" {
                walk(&p, depth + 1, out);
            }
        } else if let Some(ext) = p.extension().and_then(|x| x.to_str()) {
            if ["mp3", "m4a", "aac", "flac", "ogg", "wav"].contains(&ext.to_lowercase().as_str()) {
                out.push(p);
            }
        }
    }
}

/// Scans phone storage in the background; retries until audio permission is granted.
#[cfg(target_os = "android")]
fn spawn_scan() -> Receiver<Vec<Song>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for _ in 0..200 {
            let mut files = Vec::new();
            walk(Path::new("/storage/emulated/0"), 0, &mut files);
            if !files.is_empty() {
                files.sort();
                let v: Vec<Song> = files.iter().map(|p| read_meta(p)).collect();
                let _ = tx.send(v);
                return;
            }
            std::thread::sleep(Duration::from_secs(4));
        }
    });
    rx
}

#[cfg(not(target_os = "android"))]
fn spawn_scan() -> Receiver<Vec<Song>> {
    let (tx, rx) = channel();
    let demo: [(&str, &str, &str, f32); 12] = [
        ("Blue Sky", "Ava", "Horizon", 215.),
        ("Orange Sunset", "Nima", "Horizon", 187.),
        ("Silver Morning", "Ava", "Dawn", 242.),
        ("Glass Waves", "Rhea", "Dawn", 198.),
        ("Bright Nights", "Nima", "Dawn", 225.),
        ("A very long song title that must be truncated with ellipsis", "Some Artist With Long Name", "Long", 301.),
        ("Perspolis [Msbmusic.IR]", "Fadaei", "X", 186.),
        ("Dahan Lagh", "Dorcci", "Y", 228.),
        ("Menare", "wp.enika.ir", "Z", 264.),
        ("Ya Chi", "Arash", "W", 178.),
        ("Avaz Shodam", "Shayea", "V", 210.),
        ("Iran Iran 2", "wp.enika.ir", "U", 182.),
    ];
    let v = demo
        .iter()
        .enumerate()
        .map(|(k, (t, a, al, d))| Song { path: PathBuf::from("/demo"), title: t.to_string(), artist: a.to_string(), album: al.to_string(), dur: *d, rot: k as f32 * 72., cover: None })
        .collect();
    let _ = tx.send(v);
    rx
}

// ---------------------------------------------------------------- audio (rodio on Android, stub on desktop)

#[cfg(target_os = "android")]
mod audio {
    use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
    use std::fs::File;
    use std::io::BufReader;
    use std::path::Path;
    use std::time::Duration;

    pub struct Audio {
        _stream: OutputStream,
        handle: OutputStreamHandle,
        sink: Option<Sink>,
    }

    impl Audio {
        pub fn new() -> Option<Self> {
            let (s, h) = OutputStream::try_default().ok()?;
            Some(Self { _stream: s, handle: h, sink: None })
        }
        /// Starts the file; returns its length in seconds (0 when unknown) or None when it cannot be played.
        pub fn load(&mut self, p: &Path, vol: f32) -> Option<f32> {
            self.stop();
            let f = File::open(p).ok()?;
            let src = Decoder::new(BufReader::new(f)).ok()?;
            let sink = Sink::try_new(&self.handle).ok()?;
            let dur = src.total_duration().map_or(0., |d| d.as_secs_f32());
            sink.set_volume(vol);
            sink.append(src);
            self.sink = Some(sink);
            Some(dur)
        }
        pub fn pos(&self) -> f32 {
            self.sink.as_ref().map_or(0., |s| s.get_pos().as_secs_f32())
        }
        pub fn done(&self) -> bool {
            self.sink.as_ref().map_or(false, |s| s.empty())
        }
        pub fn pause(&self) {
            if let Some(s) = &self.sink {
                s.pause()
            }
        }
        pub fn resume(&self) {
            if let Some(s) = &self.sink {
                s.play()
            }
        }
        pub fn set_volume(&self, v: f32) {
            if let Some(s) = &self.sink {
                s.set_volume(v)
            }
        }
        pub fn seek(&self, t: f32) {
            if let Some(s) = &self.sink {
                let _ = s.try_seek(Duration::from_secs_f32(t.max(0.)));
            }
        }
        pub fn stop(&mut self) {
            if let Some(s) = self.sink.take() {
                s.stop();
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
mod audio {
    use std::path::Path;
    pub struct Audio {
        pos: f32,
    }
    impl Audio {
        pub fn new() -> Option<Self> {
            Some(Self { pos: 32. })
        }
        pub fn load(&mut self, _p: &Path, _vol: f32) -> Option<f32> {
            self.pos = 32.;
            Some(0.)
        }
        pub fn pos(&self) -> f32 {
            self.pos
        }
        pub fn done(&self) -> bool {
            false
        }
        pub fn pause(&self) {}
        pub fn resume(&self) {}
        pub fn set_volume(&self, _v: f32) {}
        pub fn seek(&self, _t: f32) {}
        pub fn stop(&mut self) {}
    }
}
use audio::Audio;

// ---------------------------------------------------------------- app

struct App {
    host: Host,
    songs: Vec<Song>,
    rx: Receiver<Vec<Song>>,
    audio: Option<Audio>,
    cur: usize,
    loaded: Option<usize>,
    playing: bool,
    shuf: bool,
    rep: u8,
    tab: Tab,
    query: String,
    dark: bool,
    pal: Pal,
    panel: bool,
    player: bool,
    vol: f32,
    seeking: Option<f32>,
    rot: f32,
    rot_mb: f32,
    fx_until: f64,
    now: f64,
    t0: Option<f64>,
    fx: Fx,
    insets: (f32, f32),
    inset_poll: f64,
    last_theme: Option<(bool, Pal)>,
    cfg_path: Option<PathBuf>,
    cov: HashMap<usize, TextureHandle>,
    list_t: f64,
    sv: f32,
    last_off: f32,
    dt: f32,
    #[cfg(not(target_os = "android"))]
    dbg: Dbg,
}

/// Where the saved settings live (the app's private folder on the phone).
fn settings_path(host: &Host) -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        host.internal_data_path().map(|p| p.join("velora.cfg"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = host;
        Some(std::env::temp_dir().join("velora.cfg"))
    }
}

fn pal_key(p: Pal) -> &'static str {
    match p {
        Pal::Pearl => "bone",
        Pal::Sky => "blue",
        Pal::Amber => "amber",
        Pal::Green => "green",
    }
}

fn load_settings(path: &Option<PathBuf>) -> (bool, Pal) {
    let (mut dark, mut pal) = (false, Pal::Pearl);
    if let Some(p) = path {
        if let Ok(text) = std::fs::read_to_string(p) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "dark" => dark = v.trim() == "1",
                        "pal" => {
                            pal = match v.trim() {
                                "blue" => Pal::Sky,
                                "amber" => Pal::Amber,
                                "green" => Pal::Green,
                                _ => Pal::Pearl,
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    (dark, pal)
}

fn save_settings(path: &Option<PathBuf>, dark: bool, pal: Pal) {
    if let Some(p) = path {
        let _ = std::fs::write(p, format!("dark={}\npal={}\n", if dark { 1 } else { 0 }, pal_key(pal)));
    }
}

/// Texture for a song's embedded cover (created on first use).
fn cover_tex(cov: &mut HashMap<usize, TextureHandle>, ctx: &Context, songs: &[Song], i: usize) -> Option<TextureId> {
    if let Some(t) = cov.get(&i) {
        return Some(t.id());
    }
    let img = songs.get(i)?.cover.as_ref()?;
    let th = ctx.load_texture(format!("cover{}", i), (**img).clone(), TextureOptions::LINEAR);
    let id = th.id();
    cov.insert(i, th);
    Some(id)
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, host: Host) -> Self {
        install_fonts(&cc.egui_ctx);
        let cfg_path = settings_path(&host);
        let (dark0, pal0) = load_settings(&cfg_path);
        let mut app = Self {
            host,
            songs: Vec::new(),
            rx: spawn_scan(),
            audio: Audio::new(),
            cur: 0,
            loaded: None,
            playing: false,
            shuf: false,
            rep: 0,
            tab: Tab::Songs,
            query: String::new(),
            dark: dark0,
            pal: pal0,
            panel: false,
            player: false,
            vol: 0.8,
            seeking: None,
            rot: 0.,
            rot_mb: 0.,
            fx_until: 0.,
            now: 0.,
            t0: None,
            fx: Fx::default(),
            insets: (0., 0.),
            inset_poll: -10.,
            last_theme: None,
            cfg_path,
            cov: HashMap::new(),
            list_t: 1.7,
            sv: 0.,
            last_off: 0.,
            dt: 0.033,
            #[cfg(not(target_os = "android"))]
            dbg: Dbg::from_env(),
        };
        #[cfg(not(target_os = "android"))]
        app.apply_dbg();
        app
    }

    fn play_index(&mut self, i: usize) {
        let Some(path) = self.songs.get(i).map(|s| s.path.clone()) else { return };
        self.cur = i;
        self.seeking = None;
        let res = self.audio.as_mut().and_then(|a| a.load(&path, self.vol));
        let ok = res.is_some();
        if let Some(d) = res {
            if d > 0. && self.songs[i].dur <= 0. {
                self.songs[i].dur = d;
            }
        }
        self.loaded = if ok { Some(i) } else { None };
        self.playing = ok;
        if ok {
            self.fx_until = self.now + 5.0;
        }
    }

    fn toggle(&mut self) {
        if self.loaded == Some(self.cur) {
            if let Some(a) = self.audio.as_ref() {
                if self.playing {
                    a.pause()
                } else {
                    a.resume()
                }
            }
            self.playing = !self.playing;
            if self.playing {
                self.fx_until = self.now + 5.0;
            }
        } else {
            self.play_index(self.cur);
        }
    }

    fn stop(&mut self) {
        if let Some(a) = self.audio.as_mut() {
            a.stop();
        }
        self.playing = false;
        self.loaded = None;
    }

    fn next(&mut self, auto: bool) {
        let n = self.songs.len();
        if n == 0 {
            return;
        }
        let mut i = if self.shuf && n > 1 {
            let j = (self.now * 1000.) as usize % n;
            if j == self.cur { (j + 1) % n } else { j }
        } else {
            self.cur + 1
        };
        if i >= n {
            if self.rep == 1 || !auto {
                i = 0;
            } else {
                self.stop();
                return;
            }
        }
        self.play_index(i);
    }

    fn pos(&self) -> f32 {
        if self.loaded == Some(self.cur) { self.audio.as_ref().map_or(0., |a| a.pos()) } else { 0. }
    }

    fn prev(&mut self) {
        if self.pos() > 3. {
            self.seek(0.);
        } else {
            let n = self.songs.len();
            if n > 0 {
                self.play_index((self.cur + n - 1) % n);
            }
        }
    }

    fn seek(&mut self, s: f32) {
        if self.loaded == Some(self.cur) {
            if let Some(a) = self.audio.as_ref() {
                a.seek(s);
            }
        }
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        (0..self.songs.len())
            .filter(|&i| {
                let s = &self.songs[i];
                q.is_empty() || format!("{} {} {}", s.title, s.artist, s.album).to_lowercase().contains(&q)
            })
            .collect()
    }

    // ------------------------------------------------------------ screens

    fn draw(&mut self, ui: &mut Ui, now: f64, st: f32) {
        let sr = ui.max_rect();
        let c = theme(self.dark, self.pal);
        let ctx = ui.ctx().clone();
        let mut g = Gfx { p: ui.painter().clone(), ctx: ctx.clone(), fx: std::mem::take(&mut self.fx), c, tint: Vec::new() };
        let ep = e_slide(ctx.animate_bool_with_time(Id::new("panel"), self.panel, 0.35));
        let el = e_slide(ctx.animate_bool_with_time(Id::new("player"), self.player, 0.4));
        let eb = e_ease(ctx.animate_bool_with_time(Id::new("back"), self.panel, 0.3));
        let on = st > 2.5 && ep < 0.01 && el < 0.01 && !self.panel && !self.player;
        g.home_bg(sr, now);
        self.home(ui, &mut g, sr, on);
        self.mini_bar(ui, &mut g, sr, on);
        if eb > 0.001 {
            g.p.rect_filled(sr, 0., rgba(0x0a1222, 0.38 * eb));
            if self.panel && ui.interact(sr, Id::new("pn_back"), Sense::click()).clicked() {
                self.panel = false;
            }
        }
        if ep > 0.001 {
            self.panel_ui(ui, &mut g, sr, ep);
        }
        if el > 0.001 {
            self.player_ui(ui, &mut g, sr, el, now);
        }
        if st < 2.5 {
            self.splash(&mut g, sr, now, st);
        }
        self.fx = std::mem::take(&mut g.fx);
    }

    fn home(&mut self, ui: &mut Ui, g: &mut Gfx, sr: Rect, on: bool) {
        let c = g.c;
        let (it, ib) = self.insets;
        let hw = sr.width().min(492.);
        let x0 = sr.center().x - hw / 2.;
        let songs_tab = self.tab == Tab::Songs;
        let idx = if songs_tab { self.filtered() } else { Vec::new() };
        let groups: Vec<(String, usize)> = if matches!(self.tab, Tab::Albums | Tab::Singers) {
            let mut m: BTreeMap<String, usize> = BTreeMap::new();
            for s in &self.songs {
                *m.entry(if self.tab == Tab::Albums { s.album.clone() } else { s.artist.clone() }).or_default() += 1;
            }
            m.into_iter().collect()
        } else {
            Vec::new()
        };
        let n_rows = if songs_tab { idx.len() } else { groups.len() };
        let list_top = it + if songs_tab { 147. } else { 118. };
        let empty = n_rows == 0;
        let list_h = if empty { 160.6 } else { n_rows as f32 * 56. };
        let content_h = (list_top + list_h + 96. + ib).max(sr.height());
        let sense = if on { Sense::click() } else { Sense::hover() };
        let mut act: Option<Act> = None;
        let mut open_panel = false;
        let mut new_tab: Option<Tab> = None;
        let now = self.now;
        let list_t = self.list_t;
        let dtv = self.dt.max(0.005);
        let bar_top = sr.bottom() - ib - 14. - 78.;
        let mut sv = self.sv;
        let mut last_off = self.last_off;

        ui.allocate_ui_at_rect(sr, |ui| {
            ScrollArea::vertical().id_source("home").enable_scrolling(on).auto_shrink([false, false]).scroll_bar_visibility(scroll_area::ScrollBarVisibility::AlwaysHidden).show_viewport(ui, |ui, vp| {
                let (full, _) = ui.allocate_exact_size(vec2(sr.width(), content_h), Sense::hover());
                let o = full.min;
                let saved = std::mem::replace(&mut g.p, ui.painter().clone());
                let at = |x: f32, y: f32| pos2(o.x + x, o.y + y);

                // scrolling speed -> rows get a soft, slightly faded look while the list is flying
                let off = vp.min.y;
                let v = ((off - last_off).abs() / dtv).min(9000.);
                sv += (v - sv) * 0.25;
                last_off = off;
                let calm = 1. - 0.3 * sstep(sv / 3500.);

                // search bar + appearance button
                let sb = Rect::from_min_size(at(x0 + 16., it + 14.), vec2(hw - 32. - 52., 44.));
                g.glass(sb, 22., 1.);
                icon(&g.p, "search", pos2(sb.left() + 15. + 11., sb.center().y), 20., c.mu);
                let te = Rect::from_min_size(pos2(sb.left() + 47., sb.top() + 1.), vec2(sb.width() - 47. - 15., 42.));
                ui.allocate_ui_at_rect(te, |ui| {
                    ui.set_enabled(on);
                    let r = ui.add(
                        TextEdit::singleline(&mut self.query)
                            .hint_text(RichText::new("Search your music").color(c.mu).font(f4(16.)))
                            .text_color(c.tx)
                            .font(f4(16.))
                            .frame(false)
                            .vertical_align(Align::Center)
                            .desired_width(te.width())
                            .min_size(vec2(te.width(), 42.)),
                    );
                    #[cfg(target_os = "android")]
                    {
                        if r.gained_focus() {
                            self.host.show_soft_input(true);
                        }
                        if r.lost_focus() {
                            self.host.hide_soft_input(false);
                        }
                    }
                    #[cfg(not(target_os = "android"))]
                    let _ = r;
                });
                let tb = Rect::from_min_size(at(x0 + hw - 16. - 42., it + 16.), vec2(40., 40.));
                let sp = g.press("theme", tb, on);
                g.glass(tb, 20., 1.);
                g.pressfx(tb, 20., sp);
                icon(&g.p, "pal", tb.center(), 20. * sp, c.tx);
                if on && ui.interact(tb, Id::new("theme"), Sense::click()).clicked() {
                    open_panel = true;
                }

                // tabs with a sliding pill
                let labels = [("Songs", Tab::Songs), ("Albums", Tab::Albums), ("Singers", Tab::Singers), ("Playlist", Tab::Playlist)];
                let mut trs: Vec<Rect> = Vec::new();
                let mut x = x0 + 16. + 2.;
                for (label, _) in labels.iter() {
                    let wt = g.p.layout_no_wrap(label.to_string(), f6(14.), c.mu).size().x + 30.;
                    trs.push(Rect::from_min_size(at(x, it + 74.), vec2(wt, 38.)));
                    x += wt + 8.;
                }
                let si = labels.iter().position(|(_, t)| *t == self.tab).unwrap_or(0);
                let px = g.ctx.animate_value_with_time(Id::new("tab_x"), trs[si].left() - o.x, 0.28) + o.x;
                let pw = g.ctx.animate_value_with_time(Id::new("tab_w"), trs[si].width(), 0.28);
                let pill = Rect::from_min_size(pos2(px, trs[si].top()), vec2(pw, 38.));
                g.p.rect_filled(pill, 19., c.card);
                g.p.rect_stroke(pill.shrink(0.5), 18.5, Stroke::new(1., wa(if c.dark { 0.12 } else { 0.7 })));
                for (i, (label, tab)) in labels.iter().enumerate() {
                    let r = trs[i];
                    txt(&g.p, r.center(), Align2::CENTER_CENTER, label, f6(14.), if self.tab == *tab { c.tx } else { c.mu });
                    if on && ui.interact(r, Id::new(("tab", i)), Sense::click()).clicked() {
                        new_tab = Some(*tab);
                    }
                }

                if songs_tab {
                    txt(&g.p, at(x0 + 20., it + 124. + 8.5), Align2::LEFT_CENTER, format!("{} {}", idx.len(), if idx.len() == 1 { "song" } else { "songs" }), f4(14.), c.mu);
                }

                // list (virtualised)
                let row_x = x0 + 16.;
                let row_w = hw - 32.;
                if empty {
                    let (h, s) = if !songs_tab {
                        if self.tab == Tab::Playlist { ("No playlists yet", "Playlists you create will appear here.") } else { ("Nothing here yet", "Your music library is empty.") }
                    } else if self.query.is_empty() {
                        ("No songs yet", "Music stored on your phone will appear here.")
                    } else {
                        ("No songs found", "Try a different search.")
                    };
                    let cx = o.x + x0 + hw / 2.;
                    txt(&g.p, pos2(cx, o.y + list_top + 48. + 17.1), Align2::CENTER_CENTER, h, f8(18.), c.tx);
                    txt(&g.p, pos2(cx, o.y + list_top + 48. + 34.2 + 15.2), Align2::CENTER_CENTER, s, f4(16.), c.mu);
                } else {
                    let first = (((vp.min.y - list_top) / 56.).floor().max(0.)) as usize;
                    let last = ((((vp.max.y - list_top) / 56.).ceil()).max(0.) as usize).min(n_rows);
                    for k in first..last {
                        let r = Rect::from_min_size(at(row_x, list_top + k as f32 * 56.), vec2(row_w, 56.));
                        let cy = r.center().y;
                        let f_top = sstep((cy - (sr.top() + it * 0.4)) / 90.);
                        let f_bot = sstep((bar_top + 40. - cy) / 130.);
                        let st_k = k.min(14) as f64;
                        let stag = sstep(((now - list_t - st_k * 0.04) / 0.35) as f32);
                        let a = (f_top * f_bot * stag * calm).clamp(0., 1.);
                        let resp = ui.interact(r, Id::new(("row", k)), sense);
                        if a < 0.01 {
                            continue;
                        }
                        let (title, sub, right, rot, cur, song) = if songs_tab {
                            let s = &self.songs[idx[k]];
                            (s.title.clone(), s.artist.clone(), if s.dur > 0. { fmt(s.dur) } else { String::new() }, s.rot, idx[k] == self.cur, Some(idx[k]))
                        } else {
                            let (nm, n) = &groups[k];
                            (nm.clone(), format!("{} {}", n, if *n == 1 { "song" } else { "songs" }), String::new(), k as f32 * 72. + 30., false, None)
                        };
                        let dx = (1. - stag) * 14.;
                        if cur {
                            g.p.rect_filled(r.translate(vec2(dx, 0.)), 18., fade(c.card, a));
                        }
                        let sc = 0.86 + 0.14 * a;
                        let art = Rect::from_center_size(pos2(r.left() + 30. + dx, cy), vec2(40. * sc, 40. * sc));
                        let cv = song.and_then(|i| cover_tex(&mut self.cov, &g.ctx, &self.songs, i));
                        if let Some(id) = cv {
                            tex_round(&g.p, id, art, 13. * sc, 0., fade(Color32::WHITE, a));
                        } else {
                            conic(&g.p, art, 13. * sc, rot, [fade(c.a1, a), fade(c.a2, a), fade(c.b3, a), fade(c.a1, a)], 96);
                        }
                        let tx = r.left() + 62. + dx;
                        let tw = if right.is_empty() { 0. } else { g.p.layout_no_wrap(right.clone(), f4(14.), c.mu).size().x + 12. };
                        let maxw = r.right() - 10. - tw - tx;
                        let t1 = ellipsize(&g.p, &title, f8(16.), maxw);
                        let t2 = ellipsize(&g.p, &sub, f4(13.333), maxw);
                        txt(&g.p, pos2(tx, cy - 8.), Align2::LEFT_CENTER, t1, f8(16.), fade(c.tx, a));
                        txt(&g.p, pos2(tx, cy + 10.), Align2::LEFT_CENTER, t2, f4(13.333), fade(c.mu, a));
                        if !right.is_empty() {
                            txt(&g.p, pos2(r.right() - 10. + dx, cy), Align2::RIGHT_CENTER, right, f4(14.), fade(c.mu, a));
                        }
                        if resp.clicked() {
                            act = Some(if songs_tab { Act::Play(idx[k]) } else { Act::Filter(groups[k].0.clone()) });
                        }
                    }
                }
                g.p = saved;
            });
        });
        self.sv = sv;
        self.last_off = last_off;
        if open_panel {
            self.panel = true;
        }
        if let Some(t) = new_tab {
            self.tab = t;
            self.list_t = now;
        }
        match act {
            Some(Act::Play(i)) => self.play_index(i),
            Some(Act::Filter(n)) => {
                self.query = n;
                self.tab = Tab::Songs;
                self.list_t = now;
            }
            None => {}
        }
    }

    fn mini_bar(&mut self, ui: &mut Ui, g: &mut Gfx, sr: Rect, on: bool) {
        let c = g.c;
        let ib = self.insets.1;
        let bw = (sr.width() - 28.).min(432.) + 22.;
        let mb = Rect::from_min_size(pos2(sr.center().x - bw / 2., sr.bottom() - ib - 14. - 78.), vec2(bw, 78.));
        g.sheet(mb, 30., mb);
        let (title, artist, rot) = self.songs.get(self.cur).map_or(("No music yet".to_string(), String::new(), 0.), |s| (s.title.clone(), s.artist.clone(), s.rot));
        let art = Rect::from_min_size(pos2(mb.left() + 11., mb.center().y - 21.), vec2(42., 42.));
        let spin = if self.playing { self.rot_mb } else { 0. };
        match cover_tex(&mut self.cov, &g.ctx, &self.songs, self.cur) {
            Some(id) => tex_round(&g.p, id, art, 21., spin, Color32::WHITE),
            None => conic(&g.p, art, 21., rot + spin.to_degrees(), [c.a1, c.a2, c.b3, c.a1], 96),
        }
        let nx = Rect::from_center_size(pos2(mb.right() - 11. - 20., mb.center().y), vec2(40., 40.));
        let pl = Rect::from_center_size(pos2(nx.left() - 8. - 20., mb.center().y), vec2(40., 40.));
        let tx = art.right() + 12.;
        let maxw = pl.left() - 8. - tx;
        let t1 = ellipsize(&g.p, &title, f8(16.), maxw);
        let t2 = ellipsize(&g.p, &artist, f4(13.333), maxw);
        txt(&g.p, pos2(tx, mb.center().y - 8.), Align2::LEFT_CENTER, t1, f8(16.), c.tx);
        txt(&g.p, pos2(tx, mb.center().y + 10.), Align2::LEFT_CENTER, t2, f4(13.333), c.mu);
        let (s1, s2) = (g.press("mb_play", pl, on), g.press("mb_next", nx, on));
        g.accent(pl, 20., false);
        g.pressfx(pl, 20., s1);
        icon(&g.p, if self.playing { "pause" } else { "play" }, pl.center(), 22. * s1, c.acon);
        g.pressfx(nx, 20., s2);
        icon(&g.p, "next", nx.center(), 22. * s2, c.tx);
        if on {
            if ui.interact(Rect::from_min_max(mb.min, pos2(pl.left() - 4., mb.bottom())), Id::new("mb_open"), Sense::click()).clicked() {
                self.player = true;
            }
            if ui.interact(pl, Id::new("mb_play"), Sense::click()).clicked() {
                self.toggle();
            }
            if ui.interact(nx, Id::new("mb_next"), Sense::click()).clicked() {
                self.next(false);
            }
        }
    }

    fn panel_ui(&mut self, ui: &mut Ui, g: &mut Gfx, sr: Rect, e: f32) {
        let c = g.c;
        let it = self.insets.0;
        let tw = sr.width().min(498.);
        let x0 = sr.center().x - tw / 2.;
        let h = 288.8 + it;
        let top = sr.top() - 1.1 * h * (1. - e);
        let pr = Rect::from_min_size(pos2(x0, top), vec2(tw, h));
        // extend upward so the (square) top corners stay off-screen while the bottom ones are rounded
        let ext = Rect::from_min_max(pos2(pr.left(), pr.top() - 40.), pr.max);
        g.sheet(ext, 32., pr);
        ui.interact(pr, Id::new("pn_bg"), Sense::click());
        let ix = pr.left() + 19.;
        let y = |dy: f32| pr.top() + it + dy;
        txt(&g.p, pos2(ix, y(34.)), Align2::LEFT_CENTER, "Appearance", f8(19.), c.tx);
        let xr = Rect::from_min_size(pos2(pr.right() - 19. - 42., y(13.)), vec2(42., 42.));
        let sx = g.press("pn_x", xr, true);
        g.pressfx(xr, 21., sx);
        icon(&g.p, "x", xr.center(), 22. * sx, c.tx);
        if ui.interact(xr, Id::new("pn_x"), Sense::click()).clicked() {
            self.panel = false;
        }
        txt(&g.p, pos2(ix, y(78.5)), Align2::LEFT_CENTER, "App color", f6(15.), c.mu);
        let cw = (tw - 38. - 30.) / 4.;
        for (i, (pl, name)) in [(Pal::Pearl, "Bone"), (Pal::Sky, "Blue"), (Pal::Amber, "Amber"), (Pal::Green, "Green")].iter().enumerate() {
            let cell = Rect::from_min_size(pos2(ix + i as f32 * (cw + 10.), y(98.)), vec2(cw, 84.8));
            let sw = Rect::from_center_size(pos2(cell.center().x, cell.top() + 8. + 21.), vec2(42., 42.));
            g.shadow(sw, 21., vec2(0., 4.), 12., c.sh);
            let pc = theme(false, *pl);
            let (lc, dc) = (pc.a1, pc.k2);
            fill(&g.p, sw, 21., &[], &|q| mix(lc, dc, lin_t(sw, 135., q)));
            g.p.circle_stroke(sw.center(), 20., Stroke::new(2., Color32::WHITE));
            let k = e_slide(g.ctx.animate_bool_with_time(Id::new(("chk", i)), self.pal == *pl, 0.28));
            if k > 0.01 {
                // the CSS check mark: an 8x14 box with a 3px right+bottom border, rotated 45deg
                let ce = sw.center() + vec2(0., -1.52);
                let (sn, cs) = (FRAC_PI_2 / 2.).sin_cos();
                let bars = |off: Vec2| -> [Vec<Pos2>; 2] {
                    let tr = |a: [(f32, f32); 4]| -> Vec<Pos2> { a.iter().map(|&(px, py)| ce + off + vec2((px * cs - py * sn) * k, (px * sn + py * cs) * k)).collect() };
                    [tr([(2.5, -8.5), (5.5, -8.5), (5.5, 8.5), (2.5, 5.5)]), tr([(-5.5, 5.5), (2.5, 5.5), (5.5, 8.5), (-5.5, 8.5)])]
                };
                for o in [0.6f32, 1.2, 1.8] {
                    for poly in bars(vec2(0., o)) {
                        g.p.add(Shape::convex_polygon(poly, rgba(0x000000, 0.2 * k), Stroke::NONE));
                    }
                }
                for poly in bars(vec2(0., 0.)) {
                    g.p.add(Shape::convex_polygon(poly, fade(Color32::WHITE, k), Stroke::NONE));
                }
            }
            txt(&g.p, pos2(cell.center().x, cell.top() + 8. + 42. + 6. + 10.4), Align2::CENTER_CENTER, *name, f6(13.), c.tx);
            if ui.interact(cell, Id::new(("pal", i)), Sense::click()).clicked() {
                self.pal = *pl;
            }
        }
        txt(&g.p, pos2(ix, y(206.3)), Align2::LEFT_CENTER, "Display mode", f6(15.), c.mu);
        let sw = (tw - 38. - 10.) / 2.;
        for (i, (name, d, ic)) in [("Light", false, "sun"), ("Dark", true, "moon")].iter().enumerate() {
            let r = Rect::from_min_size(pos2(ix + i as f32 * (sw + 10.), y(225.8)), vec2(sw, 44.));
            let sel = self.dark == *d;
            let col = if sel { c.acon } else { c.tx };
            if sel {
                g.accent(r, 22., false);
            } else {
                g.p.rect_filled(r, 22., c.card);
            }
            let tw_ = g.p.layout_no_wrap(name.to_string(), f6(16.), col).size().x;
            let gx = r.center().x - (22. + 8. + tw_) / 2.;
            icon(&g.p, ic, pos2(gx + 11., r.center().y), 22., col);
            txt(&g.p, pos2(gx + 30., r.center().y), Align2::LEFT_CENTER, *name, f6(16.), col);
            if ui.interact(r, Id::new(("mode", i)), Sense::click()).clicked() {
                self.dark = *d;
            }
        }
    }

    fn player_ui(&mut self, ui: &mut Ui, g: &mut Gfx, sr: Rect, e: f32, now: f64) {
        let c = g.c;
        let (it, ib) = self.insets;
        let pr = sr.translate(vec2(0., (1. - e) * sr.height()));
        // background: two radial glows over the usual gradient
        g.bg_linear(pr, c.bg1, c.bg2, 160.);
        g.radial(pr, 0., 1., 0.9, 0.55, c.glow, 0.7, 1.);
        g.radial(pr, 0.9, 0., 0.9, 0.55, c.b1, 0.7, 1.);
        ui.interact(pr, Id::new("pl_bg"), Sense::click());
        g.tint.clear();
        g.tint.push((pr.left_bottom() + vec2(0., -pr.height() * 0.1), pr.width() * 0.8, c.glow, 1.4));
        g.tint.push((pr.right_top() + vec2(-pr.width() * 0.1, pr.height() * 0.05), pr.width() * 0.8, c.b1, 1.2));
        let fa = sstep((e - 0.45) / 0.55);
        let w = pr.width();
        let cw = (w - 44.).min(460.);
        let x0 = pr.center().x - cw / 2.;
        let x1 = x0 + cw;
        let vh = sr.height();
        let sz = (0.74 * w).min(0.40 * vh).min(320.);
        let hs = [40., sz + 2., 57., 51., 76.];
        let avail = pr.height() - it - ib - 28.;
        let gap = ((avail - hs.iter().sum::<f32>()) / 4.).max(10.);
        let cx = pr.center().x;
        let mut y = pr.top() + it + 10.;

        // top bar
        let cl = Rect::from_min_size(pos2(x0, y), vec2(40., 40.));
        let sc = g.press("pl_close", cl, true);
        g.glass(cl, 20., 1.);
        g.pressfx(cl, 20., sc);
        icon(&g.p, "down", cl.center(), 20. * sc, c.tx);
        txt(&g.p, pos2(cx, y + 21.), Align2::CENTER_CENTER, "Now playing", f6(14.), c.mu);
        if ui.interact(cl, Id::new("pl_close"), Sense::click()).clicked() {
            self.player = false;
        }
        y += 40. + gap;

        // art: the embedded cover when the file has one, otherwise the spinning record
        let art = Rect::from_min_size(pos2(cx - (sz + 2.) / 2., y), vec2(sz + 2., sz + 2.));
        g.glass(art, 40., 1.);
        let base = self.songs.get(self.cur).map_or(0., |s| s.rot);
        match cover_tex(&mut self.cov, &g.ctx, &self.songs, self.cur) {
            Some(id) => {
                let cr = art.shrink(art.width() * 0.07);
                g.shadow(cr, 28., vec2(0., 12.), 26., fade(c.sh, fa));
                tex_round(&g.p, id, cr, 28., 0., fade(Color32::WHITE, fa));
                g.p.rect_stroke(cr.shrink(0.5), 27.5, Stroke::new(1., wa(0.5 * fa)));
            }
            None => g.disc(art.center(), sz * 0.82, base + self.rot.to_degrees(), fa),
        }
        y += sz + 2. + gap;

        // title
        let (title, artist, dur) = self.songs.get(self.cur).map_or(("No music found".to_string(), "Allow audio access to scan your phone".to_string(), 0.), |s| (s.title.clone(), s.artist.clone(), s.dur));
        let t1 = ellipsize(&g.p, &title, f8(24.), cw);
        let t2 = ellipsize(&g.p, &artist, f4(15.), cw);
        txt(&g.p, pos2(cx, y + 18.), Align2::CENTER_CENTER, t1, f8(24.), fade(c.tx, fa));
        txt(&g.p, pos2(cx, y + 38. + 9.5), Align2::CENTER_CENTER, t2, f4(15.), fade(c.mu, fa));
        y += 57. + gap;

        // seek
        let tr = Rect::from_min_size(pos2(x0, y), vec2(cw, 36.));
        let shown = self.seeking.unwrap_or_else(|| self.pos());
        let dur = dur.max(shown);
        let frac = if dur > 0. { (shown / dur).clamp(0., 1.) } else { 0. };
        g.range(tr, frac);
        txt(&g.p, pos2(x0, y + 34. + 8.5), Align2::LEFT_CENTER, fmt(shown), f4(14.), c.mu);
        txt(&g.p, pos2(x1, y + 34. + 8.5), Align2::RIGHT_CENTER, fmt(dur), f4(14.), c.mu);
        let resp = ui.interact(tr, Id::new("seek"), Sense::click_and_drag());
        if let Some(pp) = resp.interact_pointer_pos() {
            let fr = ((pp.x - tr.left() - 10.) / (tr.width() - 20.)).clamp(0., 1.);
            if resp.dragged() {
                self.seeking = Some(fr * dur);
            }
            if resp.clicked() {
                self.seek(fr * dur);
            }
        }
        if resp.drag_stopped() {
            if let Some(s) = self.seeking.take() {
                self.seek(s);
            }
        }
        y += 51. + gap;

        // controls
        let ws = [44., 54., 76., 54., 44.];
        let sp = (cw - ws.iter().sum::<f32>()) / 4.;
        let cy = y + 38.;
        let mut xx = x0;
        let mut rs = [Rect::NOTHING; 5];
        for (i, wd) in ws.iter().enumerate() {
            rs[i] = Rect::from_center_size(pos2(xx + wd / 2., cy), vec2(*wd, *wd));
            xx += wd + sp;
        }
        let ps = [g.press("c0", rs[0], true), g.press("c1", rs[1], true), g.press("c2", rs[2], true), g.press("c3", rs[3], true), g.press("c4", rs[4], true)];
        if self.shuf { g.accent(rs[0], 22., true) } else { g.glass(rs[0], 22., 1.) }
        g.pressfx(rs[0], 22., ps[0]);
        icon(&g.p, "shuf", rs[0].center(), 22. * ps[0], if self.shuf { c.acon } else { c.tx });
        g.glass(rs[1], 27., 1.);
        g.pressfx(rs[1], 27., ps[1]);
        icon(&g.p, "prev", rs[1].center(), 23. * ps[1], c.tx);
        if now < self.fx_until {
            let t = e_out((((now - (self.fx_until - 5.)) % 2.2) / 2.2) as f32);
            let s = 24. * t;
            if s > 0.05 {
                g.p.circle_stroke(rs[2].center(), 38. + s / 2., Stroke::new(s, fade(c.glow, 1. - t)));
            }
        }
        g.play_btn(rs[2]);
        g.pressfx(rs[2], 38., ps[2]);
        icon(&g.p, if self.playing { "pause" } else { "play" }, rs[2].center(), 32. * ps[2], c.acon);
        g.glass(rs[3], 27., 1.);
        g.pressfx(rs[3], 27., ps[3]);
        icon(&g.p, "next", rs[3].center(), 23. * ps[3], c.tx);
        if self.rep > 0 { g.accent(rs[4], 22., true) } else { g.glass(rs[4], 22., 1.) }
        g.pressfx(rs[4], 22., ps[4]);
        icon(&g.p, "rep", rs[4].center(), 22. * ps[4], if self.rep > 0 { c.acon } else { c.tx });
        if self.rep == 2 {
            let wd = g.p.layout_no_wrap("1".into(), f8(11.), c.acon).size().x;
            txt(&g.p, pos2(rs[4].right() - 11. - wd / 2., rs[4].top() + 9. + 6.6), Align2::CENTER_CENTER, "1", f8(11.), c.acon);
        }
        if ui.interact(rs[0], Id::new("c_shuf"), Sense::click()).clicked() {
            self.shuf = !self.shuf;
        }
        if ui.interact(rs[1], Id::new("c_prev"), Sense::click()).clicked() {
            self.prev();
        }
        if ui.interact(rs[2], Id::new("c_play"), Sense::click()).clicked() {
            self.toggle();
        }
        if ui.interact(rs[3], Id::new("c_next"), Sense::click()).clicked() {
            self.next(false);
        }
        if ui.interact(rs[4], Id::new("c_rep"), Sense::click()).clicked() {
            self.rep = (self.rep + 1) % 3;
        }
    }

    fn splash(&mut self, g: &mut Gfx, sr: Rect, now: f64, st: f32) {
        let c = g.c;
        let a = if st < 2.0 { 1. } else { 1. - e_ease(((st - 2.0) / 0.5).clamp(0., 1.)) };
        g.bg_linear(sr, fade(c.bg1, a), fade(c.bg2, a), 160.);
        g.radial(sr, 0., 1., 1.1, 0.7, c.a2, 0.55, a);
        g.radial(sr, 0.9, 0., 1.1, 0.7, c.a1, 0.55, a);
        let cx = sr.center().x;
        let top = sr.center().y - 288.78 / 2.;
        let logo = Rect::from_min_size(pos2(cx - 61., top), vec2(122., 122.));
        g.glass(logo, 61., a);
        g.disc(logo.center(), 98.4, (now as f32 % 10.) / 10. * 360., a);
        txt(&g.p, pos2(cx, top + 144. + 18.), Align2::CENTER_CENTER, APP_NAME, f8(30.), fade(c.tx, a));
        // "Written entirely in <b>Rust</b>"
        let l1a = "Written entirely in ";
        let wa_ = g.p.layout_no_wrap(l1a.into(), f4(16.), c.tx).size().x;
        let wb_ = g.p.layout_no_wrap("Rust".into(), f8(16.), c.tx).size().x;
        let sx = cx - (wa_ + wb_) / 2.;
        let py = top + 196. + 15.2;
        txt(&g.p, pos2(sx, py), Align2::LEFT_CENTER, l1a, f4(16.), fade(c.tx, a));
        txt(&g.p, pos2(sx + wa_, py), Align2::LEFT_CENTER, "Rust", f8(16.), fade(c.tx, a));
        txt(&g.p, pos2(cx, py + 30.4), Align2::CENTER_CENTER, "Fast, light and powerful", f4(16.), fade(c.tx, a));
        let bar = Rect::from_min_size(pos2(cx - 100., top + 282.78), vec2(200., 6.));
        g.p.rect_filled(bar, 3., fade(c.track, a));
        let fw = 200. * bez(0.3, 0.7, 0.2, 1., (st / 1.9).min(1.));
        if fw > 0.5 {
            let fr = Rect::from_min_size(bar.min, vec2(fw, 6.));
            let (a1, a2) = (c.a1, c.a2);
            fill(&g.p, fr, 3., &[], &|q| fade(mix(a1, a2, (q.x - fr.left()) / fw), a));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        #[allow(unused_mut)]
        let mut now = ctx.input(|i| i.time);
        #[cfg(not(target_os = "android"))]
        {
            if let Some(t) = self.dbg.time {
                now = t;
            }
        }
        let t0 = *self.t0.get_or_insert(now);
        let dt = (now - self.now).clamp(0., 0.1) as f32;
        self.dt = dt;
        self.now = now;
        while let Ok(v) = self.rx.try_recv() {
            self.songs = v;
            self.cov.clear();
            self.list_t = now.max(1.7);
        }
        if self.playing && self.audio.as_ref().map_or(false, |a| a.done()) {
            if self.rep == 2 { self.play_index(self.cur) } else { self.next(true) }
        }
        if self.playing {
            self.rot += dt * TAU / 10.;
            self.rot_mb += dt * TAU / 10.;
        } else {
            self.rot_mb = 0.;
        }
        // safe-area insets (status bar / camera cut-out / navigation bar) in points
        #[cfg(target_os = "android")]
        {
            let every = if self.insets.0 > 0. { 2.0 } else { 0.3 };
            if now - self.inset_poll > every {
                self.inset_poll = now;
                if let Some((t, b)) = query_insets(&self.host) {
                    let ppp = ctx.pixels_per_point();
                    self.insets = (t / ppp, b / ppp);
                }
            }
        }
        let th = (self.dark, self.pal);
        if self.last_theme != Some(th) {
            if self.last_theme.is_some() {
                save_settings(&self.cfg_path, self.dark, self.pal);
            }
            self.last_theme = Some(th);
            let c = theme(self.dark, self.pal);
            ctx.set_visuals(if self.dark { Visuals::dark() } else { Visuals::light() });
            ctx.style_mut(|s| {
                s.visuals.selection.bg_fill = fade(c.ac, 0.35);
                s.visuals.selection.stroke = Stroke::new(1., c.ac);
            });
        }
        let st = (now - t0) as f32;
        CentralPanel::default().frame(Frame::none()).show(ctx, |ui| self.draw(ui, now, st));
        #[cfg(not(target_os = "android"))]
        self.dbg_tick(ctx);
        // continuous animation only when something is moving; the background blobs drift slowly so keep ~30 fps
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

// ---------------------------------------------------------------- Android entry point

/// Safe-area insets in pixels: (top, bottom). Stable insets, so the on-screen keyboard does not move the UI.
#[cfg(target_os = "android")]
fn query_insets(app: &AndroidApp) -> Option<(f32, f32)> {
    use jni::objects::{JObject, JValue};
    use jni::JavaVM;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let act = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    let r = env.with_local_frame(32, |env| -> Result<(i32, i32), jni::errors::Error> {
        let win = env.call_method(&act, "getWindow", "()Landroid/view/Window;", &[])?.l()?;
        let dec = env.call_method(&win, "getDecorView", "()Landroid/view/View;", &[])?.l()?;
        let ins = env.call_method(&dec, "getRootWindowInsets", "()Landroid/view/WindowInsets;", &[])?.l()?;
        if !ins.is_null() {
            let mut t = env.call_method(&ins, "getStableInsetTop", "()I", &[])?.i()?;
            let b = env.call_method(&ins, "getStableInsetBottom", "()I", &[])?.i()?;
            if let Ok(cut) = env.call_method(&ins, "getDisplayCutout", "()Landroid/view/DisplayCutout;", &[]).and_then(|v| v.l()) {
                if !cut.is_null() {
                    if let Ok(v) = env.call_method(&cut, "getSafeInsetTop", "()I", &[]).and_then(|v| v.i()) {
                        t = t.max(v);
                    }
                }
            }
            let _ = env.exception_clear();
            return Ok((t, b));
        }
        // fall back to the resource dimensions
        let res = env.call_method(&act, "getResources", "()Landroid/content/res/Resources;", &[])?.l()?;
        let pkg = env.new_string("android")?;
        let dimen = env.new_string("dimen")?;
        let mut out = [0i32; 2];
        for (k, name) in ["status_bar_height", "navigation_bar_height"].iter().enumerate() {
            let n = env.new_string(*name)?;
            let id = env
                .call_method(&res, "getIdentifier", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I", &[JValue::Object(&n), JValue::Object(&dimen), JValue::Object(&pkg)])?
                .i()?;
            if id > 0 {
                out[k] = env.call_method(&res, "getDimensionPixelSize", "(I)I", &[JValue::Int(id)])?.i()?;
            }
        }
        Ok((out[0], out[1]))
    });
    match r {
        Ok((t, b)) => Some((t as f32, b as f32)),
        Err(_) => {
            let _ = env.exception_clear();
            None
        }
    }
}

/// Asks Android for permission to read the user's audio files (needed to list the music).
#[cfg(target_os = "android")]
fn ask_permission(app: &AndroidApp) {
    use jni::objects::{JObject, JValue};
    use jni::JavaVM;
    let run = || -> Result<(), jni::errors::Error> {
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut _) }?;
        let mut env = vm.attach_current_thread()?;
        let act = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
        let sdk = env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()?;
        let perm = if sdk >= 33 { "android.permission.READ_MEDIA_AUDIO" } else { "android.permission.READ_EXTERNAL_STORAGE" };
        let s = env.new_string(perm)?;
        let arr = env.new_object_array(1, "java/lang/String", &s)?;
        let arr_o = JObject::from(arr);
        env.call_method(&act, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&arr_o), JValue::Int(1)])?;
        Ok(())
    };
    let _ = run();
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    ask_permission(&app);
    let handle = app.clone();
    let options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(move |builder| {
            builder.with_android_app(app);
        })),
        renderer: eframe::Renderer::Wgpu,
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(App::new(cc, handle))))).unwrap();
}

// ---------------------------------------------------------------- desktop preview (not compiled for Android)

#[cfg(not(target_os = "android"))]
struct Dbg {
    state: String,
    time: Option<f64>,
    shot: Option<String>,
    frames: u32,
    asked: bool,
    play: bool,
}

#[cfg(not(target_os = "android"))]
impl Dbg {
    fn from_env() -> Self {
        let g = |k: &str| std::env::var(k).ok();
        Self {
            state: g("VELORA_STATE").unwrap_or_default(),
            time: g("VELORA_TIME").and_then(|v| v.parse().ok()),
            shot: g("VELORA_SHOT"),
            frames: 0,
            asked: false,
            play: g("VELORA_PLAY").is_some(),
        }
    }
}

#[cfg(not(target_os = "android"))]
impl App {
    fn apply_dbg(&mut self) {
        let g = |k: &str| std::env::var(k).ok();
        if let Some(v) = g("VELORA_DARK") {
            self.dark = v == "1";
        }
        if let Some(v) = g("VELORA_PAL") {
            self.pal = match v.as_str() {
                "amber" => Pal::Amber,
                "blue" => Pal::Sky,
                "green" => Pal::Green,
                _ => Pal::Pearl,
            };
        }
        if let Some(v) = g("VELORA_INSETS") {
            let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
            if p.len() == 2 {
                self.insets = (p[0], p[1]);
            }
        }
        match self.dbg.state.as_str() {
            "panel" => self.panel = true,
            "player" => self.player = true,
            _ => {}
        }
    }

    fn dbg_tick(&mut self, ctx: &Context) {
        self.dbg.frames += 1;
        if self.dbg.frames == 3 {
            while let Ok(v) = self.rx.try_recv() {
                self.songs = v;
            }
            if self.dbg.state == "player" || self.dbg.play || self.dbg.state == "homeplay" {
                self.cur = 1;
                if self.dbg.play || self.dbg.state == "homeplay" {
                    self.play_index(1);
                    self.rot = 108f32.to_radians();
                    self.rot_mb = 108f32.to_radians();
                }
            }
        }
        if self.dbg.frames == 40 && !self.dbg.asked {
            self.dbg.asked = true;
            ctx.send_viewport_cmd(ViewportCommand::Screenshot);
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let (Some(img), Some(path)) = (shot, self.dbg.shot.clone()) {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                buf.extend_from_slice(&[p.r(), p.g(), p.b(), p.a()]);
            }
            let _ = image::save_buffer(path, &buf, w as u32, h as u32, image::ColorType::Rgba8);
            std::process::exit(0);
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn run_demo() {
    let g = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (w, h, ppp) = (g("VELORA_W", 393.), g("VELORA_H", 873.), g("VELORA_PPP", 2.75));
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_inner_size([w, h]).with_resizable(false),
        renderer: eframe::Renderer::Glow,
        multisampling: 4,
        ..Default::default()
    };
    let _ = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_pixels_per_point(ppp);
            Ok(Box::new(App::new(cc, ())))
        }),
    );
}
