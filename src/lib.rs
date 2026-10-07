//! Velora 2.1 — a faithful port of Music_Player_3.html to pure Rust.
//! egui/eframe (wgpu) + rodio/symphonia + lofty. Android: native-activity + JNI.

#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

use eframe::egui::{
    self, pos2, vec2, Align2, CentralPanel, Color32, ColorImage, Context, FontData,
    FontDefinitions, FontFamily, FontId, Id, Mesh, Painter, Pos2, Rect, ScrollArea, Sense, Shape,
    Stroke, TextEdit, TextureHandle, TextureId, TextureOptions, Ui, Vec2,
};
use lofty::prelude::*;
use log::{error, info, warn};
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;
#[cfg(target_os = "android")]
type Host = AndroidApp;
#[cfg(not(target_os = "android"))]
type Host = ();

static F_REG: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static F_SEMI: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
static F_XBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-ExtraBold.ttf");
static F_VAZIR: &[u8] = include_bytes!("../assets/fonts/Vazirmatn-Regular.ttf");

const APP: &str = "Velora";

// ---------------------------------------------------------------- entry

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag(APP),
    );
    info!("boot: Android");
    let mut opts = eframe::NativeOptions::default();
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        let a = app.clone();
        opts.event_loop_builder = Some(Box::new(move |b| {
            b.with_android_app(a.clone());
        }));
    }
    start(opts, app);
}

fn start(opts: eframe::NativeOptions, host: Host) {
    init_logging();
    let res = eframe::run_native(APP, opts, Box::new(move |cc| {
        Ok(Box::new(App::new(cc, host)) as Box<dyn eframe::App>)
    }));
    if let Err(e) = res {
        error!("eframe: {e}");
    }
}

fn init_logging() {
    #[cfg(not(target_os = "android"))]
    {
        let _ = env_logger::try_init();
        info!("boot: desktop");
    }
}

// ---------------------------------------------------------------- data

#[derive(Clone)]
struct Song {
    path: PathBuf,
    title: String,
    artist: String,
    album: String,
    dur: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tab {
    Songs = 0,
    Albums,
    Singers,
    Playlist,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pal {
    Sky,
    Amber,
    Pearl,
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
    fd.font_data.insert("vazir".into(), FontData::from_static(F_VAZIR));
    let fb: Vec<String> = fd.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for name in ["inter4", "inter6", "inter8"] {
        let mut v = vec![name.to_string(), "vazir".to_string()];
        v.extend(fb.iter().cloned());
        fd.families.insert(FontFamily::Name(name.into()), v);
    }
    let mut prop = vec!["inter4".to_string(), "vazir".to_string()];
    prop.extend(fb);
    fd.families.insert(FontFamily::Proportional, prop);
    ctx.set_fonts(fd);
}

// ---------------------------------------------------------------- palette (exact CSS variables)

#[derive(Clone, Copy)]
struct C {
    bg1: Color32,
    bg2: Color32,
    tx: Color32,
    mu: Color32,
    g1: f32,
    g2: f32,
    gb: Color32,
    shb: Color32,
    sh_a: f32,
    card: Color32,
    sheet: Color32,
    gl: f32,
    bo: f32,
    a1: Color32,
    a2: Color32,
    ac: Color32,
    acon: Color32,
    k1: Color32,
    k2: Color32,
    glow: Color32,
    glow_a: f32,
    b1: Color32,
    b2: Color32,
    b3: Color32,
    track: Color32,
}

fn hx(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}
fn rgba(h: u32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (h >> 16) as u8,
        (h >> 8) as u8,
        h as u8,
        (a.clamp(0., 1.) * 255.).round() as u8,
    )
}
fn wa(a: f32) -> Color32 {
    rgba(0xffffff, a)
}
fn fade(c: Color32, a: f32) -> Color32 {
    let f = |x: u8| ((x as f32 * a.clamp(0., 1.)).round() as u8).min(255);
    Color32::from_rgba_premultiplied(f(c.r()), f(c.g()), f(c.b()), f(c.a()))
}
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0., 1.);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}
fn lin_t(rect: Rect, deg: f32, p: Pos2) -> f32 {
    let a = deg.to_radians();
    let d = vec2(a.sin(), -a.cos());
    let len = rect.width() * d.x.abs() + rect.height() * d.y.abs();
    (((p - rect.center()).dot(d)) / len.max(0.001) + 0.5).clamp(0., 1.)
}
fn fmt(s: f32) -> String {
    let s = s.max(0.).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}
fn sk(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

fn theme(dark: bool, pal: Pal) -> C {
    // accent system per palette — identical in light & dark (exactly like the CSS)
    let (a1, a2, k1, k2, ac, acon, glow, ga, b1, b2, b3, l1, l2, d1, d2, ds): (u32, u32, u32, u32, u32, u32, u32, f32, u32, u32, u32, u32, u32, u32, u32, (u8, u8, u8, u8)) = match pal {
        Pal::Sky => (0x7dd3fc, 0x2563eb, 0x2d70f0, 0x1e40af, 0x2563eb, 0xffffff, 0x2563eb, 0.42, 0x7dd3fc, 0xbfdbfe, 0xe0f2fe, 0xe4f3ff, 0xffffff, 0x0a1322, 0x0e1b33, (14, 27, 51, 230)),
        Pal::Amber => (0xfdba74, 0xea580c, 0xffb066, 0xf97316, 0xc2410c, 0x2b1204, 0xea580c, 0.42, 0xfdba74, 0xfed7aa, 0xfff1e6, 0xfff1e3, 0xffffff, 0x1c1009, 0x27170b, (39, 23, 11, 234)),
        Pal::Pearl => (0xeceff3, 0x94a3b8, 0xf6f8fb, 0xa9b4c4, 0x475569, 0x1e293b, 0x64748b, 0.40, 0xd9d3c4, 0xe9e4d8, 0xf7f3ea, 0xf1ebdf, 0xfbfaf6, 0x141416, 0x1e1e22, (30, 30, 34, 234)),
    };
    let tx = hx(if dark { 0xf1f5f9 } else { 0x0f172a });
    let mu = hx(if dark { 0xa3b3c8 } else { 0x475569 });
    let (g1, g2, gba, gl, bo) = if dark { (0.17, 0.05, 0.30, 0.16, 0.40) } else { (0.74, 0.26, 0.90, 0.50, 0.60) };
    let card = wa(if dark { 0.09 } else { 0.60 });
    let (bg1, bg2) = if dark { (hx(d1), hx(d2)) } else { (hx(l1), hx(l2)) };
    let sheet = if dark {
        Color32::from_rgba_premultiplied(
            (ds.0 as u32 * ds.3 as u32 / 255) as u8,
            (ds.1 as u32 * ds.3 as u32 / 255) as u8,
            (ds.2 as u32 * ds.3 as u32 / 255) as u8,
            ds.3,
        )
    } else {
        Color32::from_rgba_premultiplied(224, 224, 224, 224)
    };
    C {
        bg1, bg2, tx, mu, g1, g2,
        gb: wa(gba),
        shb: if dark { hx(0x000000) } else { hx(0x0f172a) },
        sh_a: if dark { 0.55 } else { 0.35 },
        card, sheet, gl, bo,
        a1: hx(a1), a2: hx(a2), ac: hx(ac), acon: hx(acon), k1: hx(k1), k2: hx(k2),
        glow: hx(glow), glow_a: ga, b1: hx(b1), b2: hx(b2), b3: hx(b3),
        track: rgba(0x808080, 0.28),
    }
}

// ---------------------------------------------------------------- painter helpers

fn outline_pts(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.).min(rect.height() / 2.).max(0.);
    let corners = [
        (rect.right() - r, rect.top() + r, -90.0f32),
        (rect.right() - r, rect.bottom() - r, 0.0),
        (rect.left() + r, rect.bottom() - r, 90.0),
        (rect.left() + r, rect.top() + r, 180.0),
    ];
    let mut v = Vec::new();
    for (cx, cy, a0) in corners {
        for k in 0..=8 {
            let a = (a0 + 90. * k as f32 / 8.).to_radians();
            v.push(pos2(cx + a.cos() * r, cy + a.sin() * r));
        }
    }
    v
}

fn grad(p: &Painter, rect: Rect, r: f32, f: &dyn Fn(Pos2) -> Color32) {
    let pts = outline_pts(rect, r);
    let c = rect.center();
    let mut m = Mesh::default();
    m.colored_vertex(c, f(c));
    for q in &pts {
        m.colored_vertex(*q, f(*q));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

fn quad_grad(p: &Painter, rect: Rect, c1: Color32, c2: Color32, deg: f32) {
    let mut m = Mesh::default();
    for q in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()] {
        m.colored_vertex(q, mix(c1, c2, lin_t(rect, deg, q)));
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(Shape::mesh(m));
}

fn radial(p: &Painter, ce: Pos2, rx: f32, ry: f32, col: Color32) {
    let mut m = Mesh::default();
    m.colored_vertex(ce, col);
    let n = 56;
    for k in 0..=n {
        let t = k as f32 / n as f32 * TAU;
        m.colored_vertex(ce + vec2(t.cos() * rx, t.sin() * ry), Color32::TRANSPARENT);
    }
    for k in 0..n as u32 {
        m.add_triangle(0, 1 + k, 2 + k);
    }
    p.add(Shape::mesh(m));
}

fn soft_shadow(p: &Painter, rect: Rect, r: f32, dy: f32, blur: f32, base: Color32, a: f32) {
    if a <= 0.005 {
        return;
    }
    for i in 1..=4u32 {
        let t = i as f32 / 4.;
        let g = blur * t;
        let aa = a * (1. - t) * (1. - t) * 1.35;
        let rr = Rect::from_center_size(
            rect.center() + vec2(0., dy * (0.4 + 0.6 * t)),
            rect.size() + vec2(g * 2., g * 2.),
        );
        p.rect_filled(rr, r + g, Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (aa.clamp(0., 1.) * 255.).round() as u8));
    }
}

fn tex_round(p: &Painter, id: TextureId, rect: Rect, r: f32) {
    let pts = outline_pts(rect, r);
    let c = rect.center();
    let uv = |q: Pos2| pos2((q.x - rect.left()) / rect.width().max(1.), (q.y - rect.top()) / rect.height().max(1.));
    let mut m = Mesh::with_texture(id);
    m.vertices.push(egui::epaint::Vertex { pos: c, uv: uv(c), color: Color32::WHITE });
    for q in &pts {
        m.vertices.push(egui::epaint::Vertex { pos: *q, uv: uv(*q), color: Color32::WHITE });
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

/// `inset 0 <off>px 0 white`: hard crescent hugging the top (top=true) or bottom edge.
fn crescent(p: &Painter, rect: Rect, r: f32, off: f32, col: Color32, top: bool) {
    let (rect, r) = (rect.shrink(1.), (r - 1.).max(0.));
    let pts = outline_pts(rect, r);
    let n = pts.len();
    if n < 3 || off <= 0.01 {
        return;
    }
    // outward normal at each outline point (points run clockwise on screen)
    let nr = |i: usize| -> Vec2 {
        let a = pts[(i + n - 1) % n];
        let b = pts[(i + 1) % n];
        let t = b - a;
        vec2(t.y, -t.x).normalized()
    };
    let depth = |i: usize| -> f32 {
        let v = nr(i);
        let f = if top { -v.y } else { v.y };
        (off * f.max(0.)).max(0.)
    };
    let mut m = Mesh::default();
    for i in 0..n {
        let q = pts[i];
        let d = depth(i);
        m.colored_vertex(q, col);
        m.colored_vertex(q - nr(i) * d, col);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        if depth(i) <= 0.01 && depth(j) <= 0.01 {
            continue;
        }
        let (a, b, c, d) = (2 * i as u32, 2 * i as u32 + 1, 2 * j as u32, 2 * j as u32 + 1);
        m.add_triangle(a, b, c);
        m.add_triangle(b, d, c);
    }
    p.add(Shape::mesh(m));
}

/// `.glass::after` — the gloss lens over the top 48%.
fn gloss(p: &Painter, rect: Rect, r: f32, a: f32) {
    if a <= 0.003 {
        return;
    }
    grad(p, rect, r, &|q| {
        let ty = (q.y - rect.top()) / rect.height().max(1.);
        wa(0.7 * (1. - (ty / 0.48).clamp(0., 1.)) * a)
    });
}

/// `.glass` — translucent gradient, hairline border, inner highlights, soft shadow.
fn glass(p: &Painter, rect: Rect, r: f32, c: C, a: f32) {
    soft_shadow(p, rect, r, 10., 15., c.shb, c.sh_a * a);
    grad(p, rect, r, &|q| wa((c.g1 + (c.g2 - c.g1) * lin_t(rect, 150., q)) * a));
    crescent(p, rect, r, 1.5, wa(0.75 * a), true);
    crescent(p, rect, r, 5., wa(0.06 * a), false);
    gloss(p, rect, r, c.gl * a);
    p.rect_stroke(rect.shrink(0.5), (r - 0.5).max(0.), sk(1.0, fade(c.gb, a)));
}

/// `--ag` accent gradient with the hard gloss step at 52%.
fn ag_fill(p: &Painter, rect: Rect, r: f32, c: C) {
    grad(p, rect, r, &|q| {
        let base = mix(c.k1, c.k2, lin_t(rect, 145., q));
        let ty = (q.y - rect.top()) / rect.height().max(1.);
        let g = if ty < 0.52 {
            0.5 + (0.06 - 0.5) * (ty / 0.52)
        } else if ty < 0.53 {
            0.06 * (1. - (ty - 0.52) / 0.01)
        } else {
            0.
        };
        mix(base, Color32::WHITE, g)
    });
}

/// `.ib.act` / `.mb .ib.go`
fn accent(p: &Painter, rect: Rect, r: f32, c: C) {
    soft_shadow(p, rect, r, 8., 10., c.glow, c.glow_a);
    ag_fill(p, rect, r, c);
    crescent(p, rect, r, 1.5, wa(0.7), true);
    p.rect_stroke(rect.shrink(0.5), (r - 0.5).max(0.), sk(1.0, wa(0.6)));
}

/// The big 74px round play button.
fn play_btn(p: &Painter, rect: Rect, c: C) {
    let r = rect.width() / 2.;
    soft_shadow(p, rect, r, 16., 18., c.glow, c.glow_a);
    ag_fill(p, rect, r, c);
    crescent(p, rect, r, 2., wa(0.8), true);
    crescent(p, rect, r, 7., wa(0.10), false);
    p.rect_stroke(rect.shrink(0.5), (r - 0.5).max(0.), sk(1.0, wa(0.75)));
}

/// Spinning vinyl: conic body, groove rings, centre hole.
fn disc(p: &Painter, ce: Pos2, d: f32, ang: f32, c: C) {
    let rect = Rect::from_center_size(ce, vec2(d, d));
    soft_shadow(p, rect, d / 2., 14., 17., c.glow, c.glow_a);
    let wedges = 72;
    let stops = [c.a1, c.a2, c.b3];
    let mut m = Mesh::default();
    m.colored_vertex(ce, c.a2);
    for k in 0..=wedges {
        let f = k as f32 / wedges as f32;
        let a = (ang + f * 360.).to_radians();
        let s = f * 3.;
        let i = (s as usize).min(2);
        m.colored_vertex(ce + vec2(a.cos(), a.sin()) * (d / 2.), mix(stops[i], stops[(i + 1) % 3], s - i as f32));
    }
    for k in 0..wedges as u32 {
        m.add_triangle(0, 1 + k, 2 + k);
    }
    p.add(Shape::mesh(m));
    let mut rr = 7.5;
    while rr < d / 2. - 2. {
        p.circle_stroke(ce, rr, sk(1.0, wa(0.16)));
        rr += 8.;
    }
    p.circle_filled(ce, d * 0.14, c.bg2);
    p.circle_stroke(ce, d * 0.14 - 1.5, sk(3.0, wa(0.8)));
}

fn txt(p: &Painter, pos: Pos2, al: Align2, s: impl AsRef<str>, font: FontId, col: Color32) -> Rect {
    p.text(pos, al, s.as_ref().to_owned(), font, col)
}

fn ellip(p: &Painter, s: &str, font: FontId, max_w: f32) -> String {
    if max_w <= 10. {
        return s.to_string();
    }
    let w = |t: &str| p.layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x;
    if w(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if w(&t) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_string() + "…"
}

// ---------------------------------------------------------------- icons

fn icon(p: &Painter, k: &str, ce: Pos2, size: f32, col: Color32) {
    let u = size / 24.;
    let m = |x: f32, y: f32| pos2(ce.x + (x - 12.) * u, ce.y + (y - 12.) * u);
    let sw = Stroke::new(2. * u, col);
    let dot = |q: Pos2| p.circle_filled(q, u, col);
    let ln = |a: Pos2, b: Pos2| {
        p.add(Shape::line(vec![a, b], sw));
        dot(a);
        dot(b);
    };
    let poly = |v: Vec<Pos2>| {
        p.add(Shape::convex_polygon(v, col, Stroke::NONE));
    };
    let path = |v: Vec<Pos2>| {
        p.add(Shape::line(v.clone(), sw));
        for q in v {
            dot(q);
        }
    };
    match k {
        "play" => poly(vec![m(8., 5.), m(19., 12.), m(8., 19.)]),
        "pause" => {
            p.rect_filled(Rect::from_min_max(m(7., 5.), m(10., 19.)), u, col);
            p.rect_filled(Rect::from_min_max(m(14., 5.), m(17., 19.)), u, col);
        }
        "prev" => {
            poly(vec![m(18., 6.), m(9., 12.), m(18., 18.)]);
            p.rect_filled(Rect::from_min_max(m(5.5, 6.), m(8., 18.)), u, col);
        }
        "next" => {
            poly(vec![m(6., 6.), m(15., 12.), m(6., 18.)]);
            p.rect_filled(Rect::from_min_max(m(16., 6.), m(18.5, 18.)), u, col);
        }
        "shuf" => {
            path(vec![m(4., 6.), m(8., 6.), m(16., 18.), m(20., 18.)]);
            path(vec![m(4., 18.), m(8., 18.), m(16., 6.), m(20., 6.)]);
            poly(vec![m(17.6, 3.6), m(21.6, 6.), m(17.6, 8.4)]);
            poly(vec![m(17.6, 15.6), m(21.6, 18.), m(17.6, 20.4)]);
        }
        "rep" => {
            path(vec![m(5., 7.), m(19., 7.)]);
            path(vec![m(19., 7.), m(19., 11.)]);
            path(vec![m(19., 17.), m(5., 17.)]);
            path(vec![m(5., 17.), m(5., 13.)]);
            poly(vec![m(16.8, 4.4), m(21., 7.), m(16.8, 9.6)]);
            poly(vec![m(7.2, 14.4), m(3., 17.), m(7.2, 19.6)]);
        }
        "search" => {
            p.circle_stroke(m(11., 11.), 6.5 * u, sw);
            ln(m(15.8, 15.8), m(20., 20.));
        }
        "theme" => {
            p.circle_stroke(m(12., 12.), 8.5 * u, sw);
            let mut v = Vec::new();
            for k in 0..=12 {
                let a = (-90. + 180. * k as f32 / 12.).to_radians();
                v.push(pos2(ce.x + a.cos() * 8.5 * u, ce.y + a.sin() * 8.5 * u));
            }
            poly(v);
        }
        "pal" => {
            p.circle_stroke(m(12., 12.), 8.5 * u, sw);
            for (x, y, r) in [(8.5, 10., 1.35), (12., 7.6, 1.35), (15.5, 10., 1.35)] {
                p.circle_filled(m(x, y), r * u, col);
            }
        }
        "x" => {
            ln(m(6., 6.), m(18., 18.));
            ln(m(18., 6.), m(6., 18.));
        }
        "vol" => {
            poly(vec![m(4., 9.), m(8., 9.), m(12.5, 5.), m(12.5, 19.), m(8., 15.), m(4., 15.)]);
            let mut v = Vec::new();
            for k in 0..=8 {
                let a = (-50. + 100. * k as f32 / 8.).to_radians();
                v.push(pos2(ce.x + 3.5 * u + a.cos() * 4.5 * u, ce.y + a.sin() * 4.5 * u));
            }
            path(v);
        }
        "down" => path(vec![m(6., 9.), m(12., 15.), m(18., 9.)]),
        "sun" => {
            p.circle_stroke(m(12., 12.), 4. * u, sw);
            for k in 0..8 {
                let a = (k as f32 * 45.).to_radians();
                ln(m(12. + 8. * a.cos(), 12. + 8. * a.sin()), m(12. + 10. * a.cos(), 12. + 10. * a.sin()));
            }
        }
        "check" => path(vec![m(5., 12.5), m(10., 17.5), m(19., 7.)]),
        _ => {}
    }
}

// ---------------------------------------------------------------- library scan

fn decode_cover(bytes: &[u8]) -> Option<Arc<ColorImage>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.resize_to_fill(160, 160, image::imageops::FilterType::Triangle).to_rgba8();
    let (w, h) = img.dimensions();
    Some(Arc::new(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw())))
}

fn read_meta(p: &Path) -> Song {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let (mut t, mut a, mut al, mut d) = (stem, "Unknown".to_string(), "Unknown".to_string(), 0.0f32);
    if let Ok(tf) = lofty::read_from_path(p) {
        d = tf.properties().duration().as_secs_f32();
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            if let Some(x) = tag.title() {
                if !x.trim().is_empty() { t = x.to_string(); }
            }
            if let Some(x) = tag.artist() {
                if !x.trim().is_empty() { a = x.to_string(); }
            }
            if let Some(x) = tag.album() {
                if !x.trim().is_empty() { al = x.to_string(); }
            }
        }
    }
    Song { path: p.to_path_buf(), title: t, artist: a, album: al, dur: d }
}

fn read_cover(path: &Path) -> Option<Arc<ColorImage>> {
    let tf = lofty::read_from_path(path).ok()?;
    let tag = tf.primary_tag().or_else(|| tf.first_tag())?;
    let pic = tag.pictures().first()?;
    decode_cover(pic.data())
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

#[cfg(target_os = "android")]
fn spawn_scan() -> Receiver<Vec<Song>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        info!("scan: waiting for storage access (/storage/emulated/0)…");
        for attempt in 0..200u32 {
            let mut files = Vec::new();
            walk(Path::new("/storage/emulated/0"), 0, &mut files);
            if !files.is_empty() {
                files.sort();
                info!("scan: {} audio files found (attempt {})", files.len(), attempt + 1);
                let t0 = std::time::Instant::now();
                let v: Vec<Song> = files.iter().map(|p| read_meta(p)).collect();
                info!("scan: tags read in {:.1}s", t0.elapsed().as_secs_f32());
                let _ = tx.send(v);
                return;
            }
            std::thread::sleep(Duration::from_secs(4));
        }
        warn!("scan: gave up after 200 attempts (no permission / no files)");
    });
    rx
}

#[cfg(not(target_os = "android"))]
fn demo_songs() -> Vec<Song> {
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
    demo.iter().enumerate().map(|(k, (t, a, al, d))| Song {
        path: PathBuf::from("/demo"),
        title: t.to_string(),
        artist: a.to_string(),
        album: al.to_string(),
        dur: *d,
    }).collect()
}

#[cfg(not(target_os = "android"))]
fn spawn_scan() -> Receiver<Vec<Song>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut files = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            walk(&PathBuf::from(home).join("Music"), 0, &mut files);
        }
        walk(&std::env::current_dir().unwrap_or_default(), 0, &mut files);
        files.sort();
        files.dedup();
        if files.is_empty() {
            info!("scan: no local audio — loading demo library");
            let _ = tx.send(demo_songs());
        } else {
            info!("scan: {} local audio files", files.len());
            let _ = tx.send(files.iter().map(|p| read_meta(p)).collect());
        }
    });
    rx
}

// ---------------------------------------------------------------- audio

mod audio {
    use log::{info, warn};
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
            match OutputStream::try_default() {
                Ok((s, h)) => {
                    info!("audio: output stream ready");
                    Some(Self { _stream: s, handle: h, sink: None })
                }
                Err(e) => {
                    warn!("audio: no output device ({e})");
                    None
                }
            }
        }
        pub fn load(&mut self, p: &Path, vol: f32) -> Option<f32> {
            self.stop();
            let f = File::open(p).ok()?;
            let src = Decoder::new(BufReader::new(f)).ok()?;
            let sink = Sink::try_new(&self.handle).ok()?;
            let dur = src.total_duration().map_or(0., |d| d.as_secs_f32());
            sink.set_volume(vol.clamp(0., 1.));
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
            if let Some(s) = &self.sink { s.pause(); }
        }
        pub fn resume(&self) {
            if let Some(s) = &self.sink { s.play(); }
        }
        pub fn set_volume(&self, v: f32) {
            if let Some(s) = &self.sink { s.set_volume(v.clamp(0., 1.)); }
        }
        pub fn seek(&self, t: f32) {
            if let Some(s) = &self.sink {
                let _ = s.try_seek(Duration::from_secs_f32(t.max(0.)));
            }
        }
        pub fn stop(&mut self) {
            if let Some(s) = self.sink.take() { s.stop(); }
        }
    }
}
use audio::Audio;

// ---------------------------------------------------------------- android: JNI helpers (crash-safe)

#[cfg(target_os = "android")]
fn query_insets(app: &AndroidApp, ppp: f32) -> (f32, f32) {
    use jni::objects::JObject;

    let read = || -> Option<(i32, i32)> {
        let raw = app.activity_as_ptr();
        if raw.is_null() {
            return None;
        }
        let cctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(cctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let act = unsafe { JObject::from_raw(raw.cast()) };
        env.push_local_frame(16).ok()?;
        let res = (|| -> Option<(i32, i32)> {
            let win = env.call_method(&act, "getWindow", "()Landroid/view/Window;", &[]).ok()?.l().ok()?;
            let dv = env.call_method(&win, "getDecorView", "()Landroid/view/View;", &[]).ok()?.l().ok()?;
            let ins = env.call_method(&dv, "getRootWindowInsets", "()Landroid/view/WindowInsets;", &[]).ok()?.l().ok()?;
            let top = env.call_method(&ins, "getSystemWindowInsetTop", "()I", &[]).ok()?.i().ok()?;
            let bot = env.call_method(&ins, "getSystemWindowInsetBottom", "()I", &[]).ok()?.i().ok()?;
            Some((top, bot))
        })();
        let _ = unsafe { env.pop_local_frame(&JObject::null()) };
        if env.exception_check().unwrap_or(true) {
            let _ = env.exception_clear();
            return None;
        }
        res
    };
    match read() {
        Some((top, bot)) => (((top as f32) / ppp).max(0.), ((bot as f32) / ppp).max(0.)),
        None => (28., 16.),
    }
}

#[cfg(target_os = "android")]
fn request_audio_permission(app: &AndroidApp) {
    use jni::objects::{JObject, JValue};

    let run = || -> Option<()> {
        let raw = app.activity_as_ptr();
        if raw.is_null() {
            return None;
        }
        let cctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(cctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let act = unsafe { JObject::from_raw(raw.cast()) };
        env.push_local_frame(16).ok()?;
        let r = (|| -> Option<()> {
            let cls = env.find_class("java/lang/String").ok()?;
            let s0 = env.new_string("android.permission.READ_MEDIA_AUDIO").ok()?;
            let s1 = env.new_string("android.permission.READ_EXTERNAL_STORAGE").ok()?;
            let arr = env.new_object_array(2, cls, s0).ok()?;
            env.set_object_array_element(&arr, 1, s1).ok()?;
            env.call_method(
                &act,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&arr), JValue::Int(4711)],
            )
            .ok()?;
            info!("perm: requestPermissions sent");
            Some(())
        })();
        let _ = unsafe { env.pop_local_frame(&JObject::null()) };
        if env.exception_check().unwrap_or(true) {
            let _ = env.exception_clear();
        }
        r
    };
    if run().is_none() {
        warn!("perm: JNI request failed — grant access manually in Settings → Apps → Velora");
    }
}

// ---------------------------------------------------------------- settings

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
        Pal::Sky => "sky",
        Pal::Amber => "amber",
        Pal::Pearl => "pearl",
    }
}

fn load_settings(path: &Option<PathBuf>) -> (bool, Pal, f32) {
    let (mut dark, mut pal, mut vol) = (false, Pal::Sky, 0.8f32);
    if let Some(p) = path {
        if let Ok(text) = std::fs::read_to_string(p) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "dark" => dark = v.trim() == "1",
                        "pal" => pal = match v.trim() {
                            "amber" => Pal::Amber,
                            "pearl" | "bone" => Pal::Pearl,
                            _ => Pal::Sky,
                        },
                        "vol" => vol = v.trim().parse::<f32>().unwrap_or(0.8).clamp(0., 1.),
                        _ => {}
                    }
                }
            }
            info!("settings: loaded ({})", p.display());
        }
    }
    (dark, pal, vol)
}

fn save_settings(path: &Option<PathBuf>, dark: bool, pal: Pal, vol: f32) {
    if let Some(p) = path {
        let s = format!("dark={}\npal={}\nvol={:.2}\n", dark as u8, pal_key(pal), vol);
        if std::fs::write(p, s).is_ok() {
            info!("settings: saved");
        }
    }
}

// ---------------------------------------------------------------- app

struct App {
    host: Host,
    songs: Vec<Song>,
    rx: Receiver<Vec<Song>>,
    cover_rx: Receiver<(usize, Option<Arc<ColorImage>>)>,
    covers: HashMap<usize, TextureHandle>,
    audio: Option<Audio>,
    cur: usize,
    loaded: Option<usize>,
    sim: bool,
    sim_pos: f32,
    playing: bool,
    shuf: bool,
    rep: u8,
    rng: u64,
    tab: Tab,
    query: String,
    dark: bool,
    pal: Pal,
    panel: bool,
    panel_t: f32,
    player: bool,
    player_t: f32,
    vol: f32,
    seeking: Option<f32>,
    rot: f32,
    song_rot: f32,
    fx_until: f64,
    now: f64,
    dt: f32,
    t0: Option<f64>,
    insets: (f32, f32),
    inset_poll: f64,
    cfg_path: Option<PathBuf>,
    albums: Vec<(String, Vec<usize>)>,
    artists: Vec<(String, Vec<usize>)>,
    got_lib: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, host: Host) -> Self {
        install_fonts(&cc.egui_ctx);
        let cfg_path = settings_path(&host);
        let (dark, pal, vol) = load_settings(&cfg_path);
        info!("boot: dark={dark} pal={pal:?} vol={vol:.2}");

        #[cfg(target_os = "android")]
        {
            let h = host.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(900));
                request_audio_permission(&h);
            });
        }

        let audio = Audio::new();
        if audio.is_none() {
            warn!("audio backend unavailable — playback will be simulated");
        }
        let (_, cover_rx) = channel();
        Self {
            host,
            songs: Vec::new(),
            rx: spawn_scan(),
            cover_rx,
            covers: HashMap::new(),
            audio,
            cur: 0,
            loaded: None,
            sim: false,
            sim_pos: 0.,
            playing: false,
            shuf: false,
            rep: 0,
            rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9_7F4A_7C15)
                | 1,
            tab: Tab::Songs,
            query: String::new(),
            dark,
            pal,
            panel: false,
            panel_t: 0.,
            player: false,
            player_t: 0.,
            vol,
            seeking: None,
            rot: 0.,
            song_rot: 0.,
            fx_until: 0.,
            now: 0.,
            dt: 1. / 60.,
            t0: None,
            insets: (0., 0.),
            inset_poll: -10.,
            cfg_path,
            albums: Vec::new(),
            artists: Vec::new(),
            got_lib: false,
        }
    }

    fn rnd(&mut self, n: usize) -> usize {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        ((x >> 11) % n as u64) as usize
    }

    fn dur(&self) -> f32 {
        self.songs.get(self.cur).map_or(0., |s| s.dur)
    }

    fn pos(&self) -> f32 {
        if let Some(s) = self.seeking {
            return s;
        }
        if self.sim {
            return self.sim_pos;
        }
        if self.loaded == Some(self.cur) {
            self.audio.as_ref().map_or(0., |a| a.pos())
        } else {
            0.
        }
    }

    fn now_meta(&self) -> (String, String) {
        match self.songs.get(self.cur) {
            Some(s) => (s.title.clone(), s.artist.clone()),
            None => ("Nothing playing".into(), "—".into()),
        }
    }

    fn cur_cover_id(&self) -> Option<TextureId> {
        self.covers.get(&self.cur).map(|t| t.id())
    }

    fn play_index(&mut self, i: usize) {
        let Some(s) = self.songs.get(i) else { return };
        let path = s.path.clone();
        let title = s.title.clone();
        self.cur = i;
        self.seeking = None;
        self.sim = false;
        self.song_rot = (i as f32 * 72.) % 360.;
        let mut ok = false;
        if path.exists() {
            if let Some(a) = self.audio.as_mut() {
                match a.load(&path, self.vol) {
                    Some(d) => {
                        if d > 0. && self.songs[i].dur <= 0. {
                            self.songs[i].dur = d;
                        }
                        ok = true;
                    }
                    None => warn!("audio: decode failed: {}", path.display()),
                }
            } else {
                warn!("audio: no backend");
            }
        } else if path.as_os_str() != "/demo" {
            warn!("audio: missing file: {}", path.display());
        }
        if ok {
            self.loaded = Some(i);
            self.playing = true;
        } else {
            self.loaded = None;
            self.sim = true;
            self.sim_pos = 0.;
            self.playing = true;
            if self.songs[i].dur <= 0. {
                self.songs[i].dur = 180.;
            }
            warn!("playback: simulating '{}'", title);
        }
        self.fx_until = self.now + 5.0;
        info!("play [{}] {}", i, title);
    }

    fn toggle(&mut self) {
        if self.songs.is_empty() {
            return;
        }
        if self.sim {
            self.playing = !self.playing;
            if self.playing {
                self.fx_until = self.now + 5.0;
            }
            return;
        }
        if self.loaded == Some(self.cur) {
            if let Some(a) = &self.audio {
                if self.playing { a.pause(); } else { a.resume(); }
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
        self.sim = false;
        self.seeking = None;
    }

    fn next(&mut self, auto: bool) {
        let n = self.songs.len();
        if n == 0 {
            return;
        }
        let mut i = if self.shuf && n > 1 {
            let j = self.rnd(n);
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

    fn prev(&mut self) {
        let n = self.songs.len();
        if n == 0 {
            return;
        }
        if self.pos() > 3. {
            self.seek(0.);
            return;
        }
        let i = (self.cur + n - 1) % n;
        self.play_index(i);
    }

    fn seek(&mut self, t: f32) {
        if self.sim {
            self.sim_pos = t;
        } else if self.loaded == Some(self.cur) {
            if let Some(a) = &self.audio {
                a.seek(t);
            }
        }
    }

    fn tick(&mut self) {
        if !self.playing {
            return;
        }
        if self.sim {
            self.sim_pos += self.dt;
            let d = self.dur();
            if d > 0. && self.sim_pos >= d {
                if self.rep == 2 { self.sim_pos = 0.; } else { self.next(true); }
            }
            return;
        }
        if self.loaded == Some(self.cur) {
            if let Some(a) = &self.audio {
                if a.done() {
                    if self.rep == 2 { self.play_index(self.cur); } else { self.next(true); }
                }
            }
        }
    }

    fn rebuild_groups(&mut self) {
        use std::collections::BTreeMap;
        let mut am: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut rm: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, s) in self.songs.iter().enumerate() {
            am.entry(s.album.clone()).or_default().push(i);
            rm.entry(s.artist.clone()).or_default().push(i);
        }
        self.albums = am.into_iter().collect();
        self.artists = rm.into_iter().collect();
        info!("library: {} albums, {} artists", self.albums.len(), self.artists.len());
    }

    fn spawn_covers(&mut self) {
        let (tx, rx) = channel();
        self.cover_rx = rx;
        let songs = self.songs.clone();
        let total = songs.len();
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            for (i, s) in songs.iter().enumerate() {
                let img = if s.path.as_os_str() == "/demo" { None } else { read_cover(&s.path) };
                if tx.send((i, img)).is_err() {
                    return;
                }
            }
            info!("covers: finished {} songs in {:.1}s", total, t0.elapsed().as_secs_f32());
        });
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        (0..self.songs.len())
            .filter(|&i| {
                q.is_empty()
                    || self.songs[i].title.to_lowercase().contains(&q)
                    || self.songs[i].artist.to_lowercase().contains(&q)
                    || self.songs[i].album.to_lowercase().contains(&q)
            })
            .collect()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(a) = self.audio.as_mut() {
            a.stop();
        }
        info!("shutdown");
    }
}

// ---------------------------------------------------------------- UI widgets

fn slider(ui: &mut Ui, rect: Rect, v: f32, c: C, interactive: bool) -> (f32, bool, bool) {
    let resp = ui.allocate_rect(rect, if interactive { Sense::click_and_drag() } else { Sense::hover() });
    let p = ui.painter();
    let t = rect.center().y;
    let tr = Rect::from_min_max(pos2(rect.left(), t - 3.), pos2(rect.right(), t + 3.));
    p.rect_filled(tr, 3., c.track);
    let mut nv = v;
    if interactive {
        if let Some(pp) = resp.interact_pointer_pos() {
            if resp.dragged() || resp.clicked() {
                nv = ((pp.x - rect.left()) / rect.width().max(1.)).clamp(0., 1.);
            }
        }
    }
    let x = rect.left() + nv * rect.width();
    if nv > 0.004 {
        let fr = Rect::from_min_max(tr.min, pos2(x, tr.max.y));
        grad(p, fr, 3., &|q| mix(c.a1, c.a2, (q.x - rect.left()) / rect.width().max(1.)));
    }
    let trect = Rect::from_center_size(pos2(x, t), vec2(20., 20.));
    soft_shadow(p, trect, 10., 3., 5., c.shb, 0.3);
    p.circle_filled(pos2(x, t), 10., Color32::WHITE);
    p.circle_stroke(pos2(x, t), 8., sk(2.0, c.ac));
    (nv, resp.dragged(), resp.drag_stopped() || resp.clicked())
}

fn blobs(p: &Painter, area: Rect, now: f64, c: C) {
    let wave = |ph: f64| -> f32 {
        let u = ((now + ph) / 14.).rem_euclid(2.);
        let k = if u < 1. { u } else { 2. - u };
        let k = k as f32;
        k * k * (3. - 2. * k)
    };
    let defs = [
        (300., pos2(area.right() - 60., area.top() + 80.), c.b1, c.bo, 0.0),
        (260., pos2(area.left() + 30., area.bottom() - 220.), c.a2, c.bo * 0.55, 5.0),
        (220., pos2(area.right() + 40., area.top() + 0.42 * area.height() + 110.), c.b2, c.bo, 9.0),
    ];
    for (d, ce0, col, op, ph) in defs {
        let k = wave(ph);
        let ce = ce0 + vec2(30. * k, -40. * k);
        let rr = d * (1. + 0.15 * k) / 2. + 30.;
        radial(p, ce, rr, rr, fade(col, op));
    }
}

fn home_col(area: Rect) -> Rect {
    let outer = area.width().min(460.);
    let ox = area.left() + (area.width() - outer) / 2.;
    Rect::from_min_max(pos2(ox + 16., area.top()), pos2(ox + outer - 16., area.bottom()))
}

impl App {
    fn draw_home_bg(&self, p: &Painter, area: Rect, c: C) {
        quad_grad(p, area, c.bg1, c.bg2, 160.);
        blobs(p, area, self.now, c);
    }

    fn draw_header(&mut self, ui: &mut Ui, col: Rect, c: C) {
        let on = self.player_t < 0.5;
        let p = ui.painter_at(col.expand(6.));
        let sbar = Rect::from_min_max(pos2(col.left(), col.top()), pos2(col.right() - 52., col.top() + 46.));
        let thm = Rect::from_min_size(pos2(col.right() - 42., col.top() + 2.), vec2(42., 42.));
        glass(&p, sbar, 23., c, 1.);
        icon(&p, "search", pos2(sbar.left() + 24., sbar.center().y), 22., c.mu);
        let edit = Rect::from_min_max(pos2(sbar.left() + 44., sbar.top() + 5.), pos2(sbar.right() - 12., sbar.bottom() - 5.));
        let q = ui.put(
            edit,
            TextEdit::singleline(&mut self.query)
                .hint_text("Search your music")
                .frame(false)
                .desired_width(edit.width())
                .font(f4(16.))
                .text_color(c.tx),
        );
        if q.changed() && self.tab != Tab::Songs {
            self.tab = Tab::Songs;
        }
        let rt = ui.allocate_rect(thm, if on { Sense::click() } else { Sense::hover() });
        icon(&p, "pal", thm.center(), 22., c.tx);
        if on && rt.clicked() {
            self.panel = !self.panel;
        }
    }

    fn draw_tabs(&mut self, ui: &mut Ui, col: Rect, c: C) {
        let on = self.player_t < 0.5;
        let p = ui.painter_at(col.expand(6.));
        let labels = ["Songs", "Albums", "Singers", "Playlist"];
        let y0 = col.top() + 58.;
        let mut x = col.left();
        for (k, lab) in labels.iter().enumerate() {
            let tw = p.layout_no_wrap((*lab).to_string(), f6(14.), Color32::WHITE).size().x;
            let w = tw + 28.;
            let r = Rect::from_min_max(pos2(x, y0), pos2(x + w, y0 + 38.));
            let sel = k == self.tab as usize;
            let resp = ui.allocate_rect(r, if on { Sense::click() } else { Sense::hover() });
            txt(&p, r.center(), Align2::CENTER_CENTER, *lab, f6(14.), if sel { c.tx } else { c.mu });
            if on && resp.clicked() {
                let nt = match k {
                    0 => Tab::Songs,
                    1 => Tab::Albums,
                    2 => Tab::Singers,
                    _ => Tab::Playlist,
                };
                if nt != self.tab {
                    self.tab = nt;
                }
            }
            x += w + 8.;
        }
    }

    fn song_row(&mut self, ui: &mut Ui, i: usize, w: f32, on: bool, c: C) {
        let (title, sub, dur) = {
            let s = &self.songs[i];
            (s.title.clone(), s.artist.clone(), s.dur)
        };
        let cov = self.covers.get(&i).map(|t| t.id());
        let is_cur = i == self.cur;
        let resp = ui.allocate_response(vec2(w, 62.), if on { Sense::click() } else { Sense::hover() });
        let r = resp.rect;
        let p = ui.painter();
        if is_cur {
            p.rect_filled(r, 18., c.card);
        }
        let mr = Rect::from_center_size(pos2(r.left() + 30., r.center().y), vec2(40., 40.));
        match cov {
            Some(id) => tex_round(p, id, mr, 13.),
            None => disc(p, mr.center(), 40., (i as f32 * 72.) % 360., c),
        }
        let tw = (w - 136.).max(40.);
        txt(p, pos2(r.left() + 62., r.center().y - 9.), Align2::LEFT_CENTER,
            ellip(p, &title, f6(15.5), tw), f6(15.5), c.tx);
        txt(p, pos2(r.left() + 62., r.center().y + 10.), Align2::LEFT_CENTER,
            ellip(p, &sub, f4(13.), tw), f4(13.), c.mu);
        if dur > 0. {
            txt(p, pos2(r.right() - 12., r.center().y), Align2::RIGHT_CENTER, fmt(dur), f4(14.), c.mu);
        }
        if on && resp.clicked() {
            self.play_index(i);
        }
    }

    fn group_row(&mut self, ui: &mut Ui, gi: usize, name: &str, count: usize, w: f32, on: bool, c: C) {
        let resp = ui.allocate_response(vec2(w, 62.), if on { Sense::click() } else { Sense::hover() });
        let r = resp.rect;
        let p = ui.painter();
        let mr = Rect::from_center_size(pos2(r.left() + 30., r.center().y), vec2(40., 40.));
        disc(p, mr.center(), 40., (gi as f32 * 72. + 30.) % 360., c);
        let tw = (w - 100.).max(40.);
        txt(p, pos2(r.left() + 62., r.center().y - 9.), Align2::LEFT_CENTER,
            ellip(p, name, f6(15.5), tw), f6(15.5), c.tx);
        txt(p, pos2(r.left() + 62., r.center().y + 10.), Align2::LEFT_CENTER,
            format!("{} song{}", count, if count == 1 { "" } else { "s" }), f4(13.), c.mu);
        if on && resp.clicked() {
            self.query = name.to_string();
            self.tab = Tab::Songs;
        }
    }

    fn draw_list(&mut self, ui: &mut Ui, col: Rect, c: C) {
        let on = self.player_t < 0.5;
        match self.tab {
            Tab::Songs => {
                let idxs = self.filtered();
                let scanning = !self.got_lib;
                let n = idxs.len();
                let cr = ui.allocate_response(vec2(col.width(), 23.), Sense::hover());
                {
                    let p = ui.painter();
                    let label = if scanning {
                        "Searching your library…".to_owned()
                    } else if self.query.trim().is_empty() {
                        format!("{} song{}", n, if n == 1 { "" } else { "s" })
                    } else {
                        format!("{} result{}", n, if n == 1 { "" } else { "s" })
                    };
                    txt(p, pos2(cr.rect.left() + 4., cr.rect.center().y), Align2::LEFT_CENTER, label, f4(14.), c.mu);
                }
                if n == 0 {
                    let (h, s) = if scanning {
                        ("Looking for music", "Music stored on your phone will appear here.")
                    } else if self.query.trim().is_empty() {
                        ("No songs yet", "Music stored on your phone will appear here.")
                    } else {
                        ("No songs found", "Try a different search.")
                    };
                    let er = ui.allocate_response(vec2(col.width(), 150.), Sense::hover());
                    let p = ui.painter();
                    txt(p, pos2(er.rect.center().x, er.rect.center().y - 12.), Align2::CENTER_CENTER, h, f6(18.), c.tx);
                    txt(p, pos2(er.rect.center().x, er.rect.center().y + 14.), Align2::CENTER_CENTER, s, f4(14.), c.mu);
                    return;
                }
                let total = n + 2; // trailing spacers so the last row clears the mini bar
                ScrollArea::vertical().id_salt("songs").show_rows(ui, 62., total, |ui, range| {
                    ui.spacing_mut().item_spacing = vec2(0., 0.);
                    let w = ui.available_width();
                    for rr in range {
                        if rr < n {
                            self.song_row(ui, idxs[rr], w, on, c);
                        } else {
                            ui.allocate_response(vec2(w, 62.), Sense::hover());
                        }
                    }
                });
            }
            Tab::Albums | Tab::Singers => {
                let is_al = self.tab == Tab::Albums;
                let src: Vec<(String, usize)> = if is_al {
                    self.albums.iter().map(|(nm, v)| (nm.clone(), v.len())).collect()
                } else {
                    self.artists.iter().map(|(nm, v)| (nm.clone(), v.len())).collect()
                };
                if src.is_empty() {
                    let er = ui.allocate_response(vec2(col.width(), 150.), Sense::hover());
                    let p = ui.painter();
                    txt(p, pos2(er.rect.center().x, er.rect.center().y - 12.), Align2::CENTER_CENTER, "Nothing here yet", f6(18.), c.tx);
                    txt(p, pos2(er.rect.center().x, er.rect.center().y + 14.), Align2::CENTER_CENTER, "Songs will be grouped once your library is scanned.", f4(14.), c.mu);
                    return;
                }
                ScrollArea::vertical().id_salt(if is_al { "albums" } else { "singers" }).show_rows(ui, 62., src.len(), |ui, range| {
                    ui.spacing_mut().item_spacing = vec2(0., 0.);
                    let w = ui.available_width();
                    for rr in range {
                        self.group_row(ui, rr, &src[rr].0, src[rr].1, w, on, c);
                    }
                });
            }
            Tab::Playlist => {
                let er = ui.allocate_response(vec2(col.width(), 150.), Sense::hover());
                let p = ui.painter();
                txt(p, pos2(er.rect.center().x, er.rect.center().y - 12.), Align2::CENTER_CENTER, "No playlists yet", f6(18.), c.tx);
                txt(p, pos2(er.rect.center().x, er.rect.center().y + 14.), Align2::CENTER_CENTER, "Playlists you create will appear here.", f4(14.), c.mu);
            }
        }
    }

    fn draw_mini(&mut self, ui: &mut Ui, area: Rect, c: C) {
        let on = self.player_t < 0.5;
        let bottom = area.bottom() - 14.;
        let w = (area.width() - 28.).min(432.);
        let mini = Rect::from_min_max(pos2(area.center().x - w / 2., bottom - 60.), pos2(area.center().x + w / 2., bottom));
        let p = ui.painter_at(mini.expand(26.));
        soft_shadow(&p, mini, 30., 10., 15., c.shb, c.sh_a);
        grad(&p, mini, 30., &|_| c.sheet);
        crescent(&p, mini, 30., 1.5, wa(0.75), true);
        gloss(&p, mini, 30., c.gl);
        p.rect_stroke(mini.shrink(0.5), 29.5, sk(1.0, c.gb));

        let cy = mini.center().y;
        let art_c = pos2(mini.left() + 31., cy);
        let (title, artist) = self.now_meta();
        let cov = self.cur_cover_id();
        let angle = (self.song_rot + self.rot) % 360.;
        let pb = Rect::from_center_size(pos2(mini.right() - 30., cy), vec2(40., 40.));
        let nb = Rect::from_center_size(pos2(mini.right() - 78., cy), vec2(40., 40.));
        let open_r = Rect::from_min_max(pos2(mini.left() + 2., mini.top()), pos2(nb.left() - 6., mini.bottom()));
        let ropen = ui.allocate_rect(open_r, if on { Sense::click() } else { Sense::hover() });
        let rnb = ui.allocate_rect(nb, if on { Sense::click() } else { Sense::hover() });
        let rpb = ui.allocate_rect(pb, if on { Sense::click() } else { Sense::hover() });
        if on && rpb.clicked() { self.toggle(); }
        if on && rnb.clicked() { self.next(false); }
        if on && ropen.clicked() { self.player = true; }

        match cov {
            Some(id) => tex_round(&p, id, Rect::from_center_size(art_c, vec2(42., 42.)), 21.),
            None => disc(&p, art_c, 42., angle, c),
        }
        let tw = (nb.left() - art_c.x - 42.).max(30.);
        txt(&p, pos2(art_c.x + 33., cy - 9.), Align2::LEFT_CENTER,
            ellip(&p, &title, f6(15.), tw), f6(15.), c.tx);
        txt(&p, pos2(art_c.x + 33., cy + 10.), Align2::LEFT_CENTER,
            ellip(&p, &artist, f4(12.5), tw), f4(12.5), c.mu);
        accent(&p, pb, 20., c);
        icon(&p, if self.playing { "pause" } else { "play" }, pb.center(), 20., c.acon);
        icon(&p, "next", nb.center(), 22., c.tx);
    }

    fn draw_player(&mut self, ui: &mut Ui, area: Rect, c: C) {
        let t = self.player_t;
        if t < 0.004 {
            return;
        }
        let on = t > 0.95;
        let top = area.top() + (1. - t) * area.height();
        let p = ui.painter_at(area);
        let reg = Rect::from_min_max(pos2(area.left(), top), area.max);
        quad_grad(&p, reg, c.bg1, c.bg2, 160.);
        radial(&p, pos2(area.left() + 0.9 * area.width(), top), 0.63 * area.width(), 0.385 * area.height(), fade(c.b1, 0.55));
        radial(&p, pos2(area.left(), area.bottom()), 0.63 * area.width(), 0.385 * area.height(), fade(c.glow, c.glow_a * 0.9));

        let outer = area.width().min(460.);
        let ox = area.left() + (area.width() - outer) / 2.;
        let cl = ox + 22.;
        let cr_ = ox + outer - 22.;

        // top bar
        let xb = Rect::from_center_size(pos2(cl + 21., top + 31.), vec2(42., 42.));
        let rxb = ui.allocate_rect(xb, if on { Sense::click() } else { Sense::hover() });
        glass(&p, xb, 21., c, 1.);
        icon(&p, "down", xb.center(), 22., c.tx);
        txt(&p, pos2(area.center().x, xb.center().y), Align2::CENTER_CENTER, "Now playing", f6(14.), c.mu);
        if on && rxb.clicked() {
            self.player = false;
        }

        // layout anchored to the bottom (flex justify-between in the HTML)
        let ty = area.bottom() - 245.; // title centre
        let srect = Rect::from_min_max(pos2(cl, ty + 46.), pos2(cr_, ty + 82.));
        let cy = ty + 138.;
        let vy = ty + 201.;
        let art_bottom = ty - 26.;

        // art
        let s = ((cr_ - cl) * 0.66).min(art_bottom - (top + 62.)).min(area.height() * 0.36).min(280.).max(110.);
        let acc = pos2(area.center().x, art_bottom - s / 2.);
        let arect = Rect::from_center_size(acc, vec2(s, s));
        match self.cur_cover_id() {
            Some(id) => {
                soft_shadow(&p, arect, 40., 12., 16., c.shb, c.sh_a);
                tex_round(&p, id, arect, 40.);
                p.rect_stroke(arect.shrink(0.5), 39.5, sk(1.0, c.gb));
            }
            None => {
                glass(&p, arect, 40., c, 1.);
                disc(&p, acc, s * 0.82, (self.song_rot + self.rot) % 360., c);
            }
        }

        // meta
        let d = self.dur().max(0.001);
        let pos = self.pos().clamp(0., d);
        let (title, artist) = self.now_meta();
        txt(&p, pos2(area.center().x, ty), Align2::CENTER_CENTER,
            ellip(&p, &title, f8(24.), area.width() - 48.), f8(24.), c.tx);
        txt(&p, pos2(area.center().x, ty + 28.), Align2::CENTER_CENTER,
            ellip(&p, &artist, f4(15.), area.width() - 48.), f4(15.), c.mu);

        // seek
        let (nv, dragging, done) = slider(ui, srect, pos / d, c, on);
        if on {
            if dragging {
                self.seeking = Some(nv * d);
            } else if done {
                self.seek(nv * d);
            }
        }
        txt(&p, pos2(cl + 2., srect.bottom() + 9.), Align2::LEFT_CENTER,
            fmt(if dragging { nv * d } else { pos }), f4(14.), c.mu);
        txt(&p, pos2(cr_ - 2., srect.bottom() + 9.), Align2::RIGHT_CENTER, fmt(d), f4(14.), c.mu);

        // controls
        let fr = |f: f32| cl + (cr_ - cl) * f;
        let shb = Rect::from_center_size(pos2(fr(0.10), cy), vec2(42., 42.));
        let pvb = Rect::from_center_size(pos2(fr(0.30), cy), vec2(52., 52.));
        let pb = Rect::from_center_size(pos2(fr(0.50), cy), vec2(74., 74.));
        let nxb = Rect::from_center_size(pos2(fr(0.70), cy), vec2(52., 52.));
        let rb = Rect::from_center_size(pos2(fr(0.90), cy), vec2(42., 42.));

        let rs = ui.allocate_rect(shb, if on { Sense::click() } else { Sense::hover() });
        if self.shuf { accent(&p, shb, 21., c); } else { glass(&p, shb, 21., c, 1.); }
        icon(&p, "shuf", shb.center(), 22., if self.shuf { c.acon } else { c.tx });
        if on && rs.clicked() { self.shuf = !self.shuf; }

        let rp = ui.allocate_rect(pvb, if on { Sense::click() } else { Sense::hover() });
        glass(&p, pvb, 26., c, 1.);
        icon(&p, "prev", pvb.center(), 24., c.tx);
        if on && rp.clicked() { self.prev(); }

        if self.now < self.fx_until {
            let u = ((self.now % 2.2) / 2.2) as f32;
            p.circle_filled(pb.center(), 37. + u * 24., fade(c.glow, c.glow_a * (1. - u) * 0.8));
        }
        let rpb2 = ui.allocate_rect(pb, if on { Sense::click() } else { Sense::hover() });
        play_btn(&p, pb, c);
        icon(&p, if self.playing { "pause" } else { "play" }, pb.center(), 30., c.acon);
        if on && rpb2.clicked() { self.toggle(); }

        let rn = ui.allocate_rect(nxb, if on { Sense::click() } else { Sense::hover() });
        glass(&p, nxb, 26., c, 1.);
        icon(&p, "next", nxb.center(), 24., c.tx);
        if on && rn.clicked() { self.next(false); }

        let rr = ui.allocate_rect(rb, if on { Sense::click() } else { Sense::hover() });
        if self.rep > 0 { accent(&p, rb, 21., c); } else { glass(&p, rb, 21., c, 1.); }
        icon(&p, "rep", rb.center(), 22., if self.rep > 0 { c.acon } else { c.tx });
        if self.rep == 2 {
            txt(&p, pos2(rb.right() - 9., rb.top() + 10.), Align2::CENTER_CENTER, "1", f8(11.), c.acon);
        }
        if on && rr.clicked() { self.rep = (self.rep + 1) % 3; }

        // volume
        icon(&p, "vol", pos2(cl + 10., vy), 20., c.mu);
        let vw = (cr_ - cl - 66.).max(60.);
        let vrect = Rect::from_min_max(pos2(cl + 46., vy - 18.), pos2(cl + 46. + vw, vy + 18.));
        let (nv, _dg, done) = slider(ui, vrect, self.vol, c, on);
        if on {
            if (nv - self.vol).abs() > 0.001 {
                self.vol = nv;
                if let Some(a) = &self.audio {
                    a.set_volume(nv);
                }
            }
            if done {
                save_settings(&self.cfg_path, self.dark, self.pal, self.vol);
            }
        }
    }

    fn draw_panel(&mut self, ui: &mut Ui, area: Rect, c: C) {
        let t = self.panel_t;
        if t < 0.004 {
            return;
        }
        let on = t > 0.95;
        let p = ui.painter_at(area);
        p.rect_filled(area, 0., fade(hx(0x0a1222), 0.38 * t));
        let back = ui.allocate_rect(area, Sense::click());
        if on && back.clicked() {
            self.panel = false;
        }

        let pw = area.width().min(460.);
        let px0 = area.left() + (area.width() - pw) / 2.;
        let ph = 262. + self.insets.0;
        let vis_top = area.top() - self.insets.0;
        let slide = (1. - t) * (ph + 30.);
        let rect = Rect::from_min_max(pos2(px0, vis_top - 44. - slide), pos2(px0 + pw, vis_top - 44. - slide + ph + 44.));

        soft_shadow(&p, Rect::from_min_max(pos2(rect.left(), vis_top), pos2(rect.right(), rect.bottom())), 32., 10., 15., c.shb, c.sh_a);
        grad(&p, rect, 32., &|_| c.sheet);
        crescent(&p, rect, 32., 1.5, wa(0.6), false);
        gloss(&p, rect, 32., c.gl);
        p.rect_stroke(rect.shrink(0.5), 31.5, sk(1.0, c.gb));

        let hy = vis_top + self.insets.0 + 12. + 21.;
        txt(&p, pos2(px0 + 18., hy), Align2::LEFT_CENTER, "Appearance", f8(19.), c.tx);
        let xb = Rect::from_center_size(pos2(px0 + pw - 39., hy), vec2(42., 42.));
        let rxb = ui.allocate_rect(xb, if on { Sense::click() } else { Sense::hover() });
        glass(&p, xb, 21., c, 1.);
        icon(&p, "x", xb.center(), 20., c.tx);
        if on && rxb.clicked() {
            self.panel = false;
        }

        let y0 = hy + 37.;
        txt(&p, pos2(px0 + 18., y0), Align2::LEFT_CENTER, "App color", f6(15.), c.mu);
        let sw_names = ["Blue & White", "Orange & White", "Bone & Silver"];
        let sw_pals = [Pal::Sky, Pal::Amber, Pal::Pearl];
        let sw_cols = [(0xbae6fdu32, 0x2563ebu32), (0xfed7aa, 0xea580c), (0xf7f3ea, 0x94a3b8)];
        let cell = pw / 3.;
        for k in 0..3 {
            let cx = px0 + cell * k as f32 + cell / 2.;
            let cir = Rect::from_center_size(pos2(cx, y0 + 37.), vec2(38., 38.));
            let resp = ui.allocate_rect(cir.expand(6.), if on { Sense::click() } else { Sense::hover() });
            soft_shadow(&p, cir, 19., 4., 6., c.shb, c.sh_a);
            grad(&p, cir, 19., &|q| mix(hx(sw_cols[k].0), hx(sw_cols[k].1), lin_t(cir, 135., q)));
            p.rect_stroke(cir.shrink(1.), 18., sk(2.0, Color32::WHITE));
            if sw_pals[k] == self.pal {
                icon(&p, "check", cir.center(), 20., Color32::WHITE);
            }
            txt(&p, pos2(cx, y0 + 71.), Align2::CENTER_CENTER, sw_names[k], f6(13.), c.tx);
            if on && resp.clicked() {
                self.pal = sw_pals[k];
                save_settings(&self.cfg_path, self.dark, self.pal, self.vol);
            }
        }

        let y1 = y0 + 104.;
        txt(&p, pos2(px0 + 18., y1), Align2::LEFT_CENTER, "Display mode", f6(15.), c.mu);
        let bw = (pw - 46.) / 2.;
        for (k2, (lab, is_dark)) in [("Light", false), ("Dark", true)].iter().enumerate() {
            let bx = px0 + 18. + k2 as f32 * (bw + 10.);
            let br = Rect::from_min_max(pos2(bx, y1 + 14.), pos2(bx + bw, y1 + 58.));
            let resp = ui.allocate_rect(br, if on { Sense::click() } else { Sense::hover() });
            let sel = self.dark == *is_dark;
            if sel { accent(&p, br, 22., c); } else { p.rect_filled(br, 22., c.card); }
            icon(&p, if *is_dark { "theme" } else { "sun" }, pos2(br.center().x - 30., br.center().y), 20., if sel { c.acon } else { c.tx });
            txt(&p, pos2(br.center().x + 8., br.center().y), Align2::CENTER_CENTER, *lab, f6(15.), if sel { c.acon } else { c.tx });
            if on && resp.clicked() {
                self.dark = *is_dark;
                save_settings(&self.cfg_path, self.dark, self.pal, self.vol);
            }
        }
    }

    fn draw_splash(&mut self, ui: &mut Ui, area: Rect, c: C) {
        let t0 = self.t0.unwrap_or(self.now);
        let t = ((self.now - t0) as f32).max(0.);
        if t > 2.5 {
            return;
        }
        let p = ui.painter_at(area);
        quad_grad(&p, area, c.bg1, c.bg2, 160.);
        radial(&p, pos2(area.left() + 0.9 * area.width(), area.top()), 0.605 * area.width(), 0.385 * area.height(), c.a1);
        radial(&p, pos2(area.left(), area.bottom()), 0.605 * area.width(), 0.385 * area.height(), c.a2);

        let cx = area.center().x;
        let cy = area.center().y - 40.;
        let lg = Rect::from_center_size(pos2(cx, cy - 60.), vec2(120., 120.));
        glass(&p, lg, 60., c, 1.);
        disc(&p, pos2(cx, cy - 60.), 98., (self.now * 36.0) as f32 % 360., c);
        txt(&p, pos2(cx, cy + 22.), Align2::CENTER_CENTER, "Music Player", f8(30.), c.tx);
        txt(&p, pos2(cx, cy + 60.), Align2::CENTER_CENTER, "Written entirely in Rust", f6(16.), c.tx);
        txt(&p, pos2(cx, cy + 88.), Align2::CENTER_CENTER, "Fast, light and powerful", f4(16.), c.mu);

        let br = Rect::from_center_size(pos2(cx, cy + 122.), vec2(200., 6.));
        p.rect_filled(br, 3., c.track);
        let fw = ((t / 1.9).clamp(0., 1.)) * 200.;
        if fw > 1. {
            let fill = Rect::from_min_max(br.min, pos2(br.min.x + fw, br.max.y));
            grad(&p, fill, 3., &|q| mix(c.a1, c.a2, ((q.x - br.min.x) / 200.).clamp(0., 1.)));
        }

        if t > 2.2 {
            let u = ((t - 2.2) / 0.3).clamp(0., 1.);
            p.rect_filled(area, 0., fade(c.bg1, u));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|i| i.time);
        self.dt = ctx.input(|i| i.stable_dt).max(1. / 120.);
        if self.t0.is_none() {
            self.t0 = Some(self.now);
        }
        self.player_t = ctx.animate_value_with_time(Id::new("player"), if self.player { 1. } else { 0. }, 0.4);
        self.panel_t = ctx.animate_value_with_time(Id::new("panel"), if self.panel { 1. } else { 0. }, 0.35);

        if !self.got_lib {
            if let Ok(v) = self.rx.try_recv() {
                self.got_lib = true;
                info!("library: ready — {} songs", v.len());
                self.songs = v;
                self.covers.clear();
                self.rebuild_groups();
                self.spawn_covers();
            }
        }
        let mut up = 0;
        while up < 8 {
            match self.cover_rx.try_recv() {
                Ok((i, Some(img))) => {
                    let th = ctx.load_texture(format!("cov{i}"), (*img).clone(), TextureOptions::LINEAR);
                    self.covers.insert(i, th);
                    up += 1;
                }
                Ok((_, None)) => up += 1,
                Err(_) => break,
            }
        }
        if self.covers.len() > 400 {
            warn!("covers: cache overflow — clearing");
            self.covers.clear();
        }

        #[cfg(target_os = "android")]
        {
            if self.now > self.inset_poll + 2. {
                self.inset_poll = self.now;
                let ni = query_insets(&self.host, ctx.pixels_per_point());
                if (ni.0 - self.insets.0).abs() > 0.5 || (ni.1 - self.insets.1).abs() > 0.5 {
                    info!("insets: top={:.0} bottom={:.0}", ni.0, ni.1);
                    self.insets = ni;
                }
            }
        }

        self.tick();
        if self.playing {
            self.rot = (self.rot + self.dt * 36.) % 360.;
        }

        let c = theme(self.dark, self.pal);

        CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let sr = ui.max_rect();
            let area = Rect::from_min_max(
                pos2(sr.left(), sr.top() + self.insets.0),
                pos2(sr.right(), sr.bottom() - self.insets.1),
            );
            let bg = ui.painter_at(area.expand(2.));
            self.draw_home_bg(&bg, area, c);
            let col = home_col(area);
            self.draw_header(ui, col, c);
            self.draw_tabs(ui, col, c);
            self.draw_list(ui, col, c);
            if self.player_t < 0.98 {
                self.draw_mini(ui, area, c);
            }
            self.draw_player(ui, area, c);
            self.draw_panel(ui, area, c);
            self.draw_splash(ui, area, c);
        });

        ctx.request_repaint_after(Duration::from_millis(33));
    }
}
