//! Velora 2.0 — glass music player, pure Rust.
//! egui/eframe (wgpu) + rodio/symphonia + lofty. Android: native-activity + JNI insets.

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
    Pearl,
    Sky,
    Amber,
    Green,
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

// ---------------------------------------------------------------- palette

#[derive(Clone, Copy)]
struct C {
    bg1: Color32,
    bg2: Color32,
    tx: Color32,
    mu: Color32,
    a1: Color32,
    a2: Color32,
    b3: Color32,
    k1: Color32,
    k2: Color32,
    ac: Color32,
    acon: Color32,
    glow: Color32,
    glow_a: f32,
    shb: Color32,
    sh_a: f32,
    gb: Color32,
    sheet: Color32,
    track: Color32,
    hl: Color32,
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
/// f32-typed stroke constructor (keeps the compiler from f64-literal fallback warnings).
fn sk(w: f32, c: Color32) -> Stroke {
    Stroke::new(w, c)
}

fn theme(dark: bool, pal: Pal) -> C {
    let (a1, a2, b1, b2, b3, k1, k2, ac, acon, glow, ga, l1, l2, d1, d2): (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, f32, u32, u32, u32, u32) = match pal {
        Pal::Pearl => (0xeceff3, 0x94a3b8, 0xd9d3c4, 0xe9e4d8, 0xf7f3ea, 0xf4f6fa, 0x98a5b8, 0x475569, 0x1e293b, 0x64748b, 0.36, 0xf1ebdf, 0xfbfaf6, 0x141416, 0x1e1e22),
        Pal::Sky => (0xbcd7fa, 0x5f8be0, 0xcfe2fb, 0xe0ecfc, 0xf2f7fe, 0x78a4f2, 0x4a73d6, 0x3f68d4, 0xffffff, 0x4a73d6, 0.32, 0xe8f0fa, 0xfbfdff, 0x0b1322, 0x14213a),
        Pal::Amber => (0xf8d3a6, 0xe58f45, 0xf7d9b4, 0xfae6cd, 0xfdf4e8, 0xf5b06a, 0xe0802f, 0xb4601c, 0x3b1d07, 0xe0802f, 0.32, 0xfbefe0, 0xfffaf3, 0x1a110a, 0x2a1b10),
        Pal::Green => (0xbfe3cd, 0x56a67c, 0xcdebd8, 0xe1f3e8, 0xf3fbf6, 0x6cc096, 0x3a8c63, 0x2f7d57, 0xffffff, 0x3a8c63, 0.32, 0xe7f3ea, 0xfafdfb, 0x0a1510, 0x12231b),
    };
    let (bg1, bg2) = if dark { (hx(d1), hx(d2)) } else { (hx(l1), hx(l2)) };
    let warm = pal == Pal::Pearl;
    let tx = hx(match (dark, warm) {
        (false, false) => 0x0f172a,
        (false, true) => 0x1d1c1a,
        (true, false) => 0xf1f5f9,
        (true, true) => 0xf2f0ec,
    });
    let mu = hx(match (dark, warm) {
        (false, false) => 0x475569,
        (false, true) => 0x625e56,
        (true, false) => 0xa3b3c8,
        (true, true) => 0xaaa69d,
    });
    let m = if dark { mix(bg2, hx(0xffffff), 0.05) } else { bg2 };
    let sheet = Color32::from_rgba_unmultiplied(m.r(), m.g(), m.b(), 246);
    let bl = if dark {
        [mix(bg2, hx(a2), 0.5), mix(bg2, hx(a2), 0.38), mix(bg2, hx(a1), 0.28)]
    } else {
        [hx(b1), hx(a2), hx(b2)]
    };
    C {
        bg1,
        bg2,
        tx,
        mu,
        a1: hx(a1),
        a2: hx(a2),
        b3: hx(b3),
        k1: hx(k1),
        k2: hx(k2),
        ac: hx(ac),
        acon: hx(acon),
        glow: hx(glow),
        glow_a: ga,
        shb: if dark { hx(0x000000) } else { hx(0x0f172a) },
        sh_a: if dark { 0.55 } else { 0.35 },
        gb: wa(if dark { 0.22 } else { 0.85 }),
        sheet,
        track: if dark { wa(0.16) } else { rgba(0x0f172a, 0.12) },
        hl: if dark { wa(0.07) } else { rgba(0x0f172a, 0.05) },
        bl,
        blo: if dark { [0.5, 0.4, 0.32] } else { [0.55, 0.3, 0.5] },
        g1: if dark { 0.12 } else { 0.72 },
        g2: if dark { 0.035 } else { 0.25 },
        gl: if dark { 0.10 } else { 0.5 },
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

fn glass(p: &Painter, rect: Rect, r: f32, c: C, a: f32) {
    soft_shadow(p, rect, r, 5., 14., c.shb, c.sh_a * a);
    grad(p, rect, r, &|q| {
        let mut al = (c.g1 + (c.g2 - c.g1) * lin_t(rect, 150., q)) * a;
        let ty = (q.y - rect.top()) / rect.height().max(1.);
        if ty < 0.48 {
            al += 0.45 * (1. - ty / 0.48) * c.gl * a;
        }
        wa(al.min(1.))
    });
    p.rect_stroke(rect.shrink(0.5), (r - 0.5).max(0.), sk(1.0, fade(c.gb, a)));
}

fn accent(p: &Painter, rect: Rect, r: f32, c: C) {
    soft_shadow(p, rect, r, 5., 12., c.glow, c.glow_a);
    grad(p, rect, r, &|q| {
        let base = mix(c.k1, c.k2, lin_t(rect, 145., q));
        let ty = (q.y - rect.top()) / rect.height().max(1.);
        mix(base, Color32::WHITE, if ty < 0.5 { 0.30 * (1. - ty / 0.5) } else { 0. })
    });
    p.rect_stroke(rect.shrink(0.5), (r - 0.5).max(0.), sk(1.0, wa(0.55)));
}

fn disc(p: &Painter, ce: Pos2, d: f32, rot: f32, c: C) {
    let rect = Rect::from_center_size(ce, vec2(d, d));
    soft_shadow(p, rect, d / 2., 10., 20., c.shb, 0.4);
    let wedges = 72;
    let stops = [c.a1, c.a2, c.b3];
    let mut m = Mesh::default();
    m.colored_vertex(ce, c.a2);
    for k in 0..=wedges {
        let f = k as f32 / wedges as f32;
        let ang = (rot + f * 360.).to_radians();
        let s = f * 3.;
        let i = (s as usize).min(2);
        let col = mix(stops[i], stops[(i + 1) % 3], s - i as f32);
        m.colored_vertex(ce + vec2(ang.cos(), ang.sin()) * (d / 2.), col);
    }
    for k in 0..wedges as u32 {
        m.add_triangle(0, 1 + k, 2 + k);
    }
    p.add(Shape::mesh(m));
    let mut rr = d * 0.16 + 4.;
    while rr < d / 2. - 3. {
        p.circle_stroke(ce, rr, sk(1.0, wa(0.10)));
        rr += 7.;
    }
    p.circle_filled(ce, d * 0.13, c.bg2);
    p.circle_stroke(ce, d * 0.13 + 1., sk(2.0, wa(0.7)));
}

fn tile(p: &Painter, rect: Rect, r: f32, c: C, letter: Option<char>) {
    grad(p, rect, r, &|q| {
        let base = mix(c.k1, c.k2, lin_t(rect, 135., q));
        let ty = (q.y - rect.top()) / rect.height().max(1.);
        mix(base, Color32::WHITE, if ty < 0.5 { 0.22 * (1. - ty / 0.5) } else { 0. })
    });
    if let Some(ch) = letter {
        txt(p, rect.center(), Align2::CENTER_CENTER, ch.to_string(), f8(rect.height() * 0.4), c.acon);
    }
}
fn tile_letter(s: &str) -> Option<char> {
    s.chars().next().filter(|ch| ch.is_ascii_alphabetic()).map(|ch| ch.to_ascii_uppercase())
}

fn txt(p: &Painter, pos: Pos2, al: Align2, s: impl AsRef<str>, font: FontId, col: Color32) -> Rect {
    p.text(pos, al, s.as_ref().to_owned(), font, col)
}

fn ellip(p: &Painter, s: &str, font: FontId, max_w: f32) -> String {
    if max_w <= 0. {
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
        "note" => {
            path(vec![m(9., 17.5), m(9., 6.), m(17., 5.), m(17., 14.)]);
            p.circle_filled(m(7., 17.5), 2.1 * u, col);
            p.circle_filled(m(15., 14.), 2.1 * u, col);
        }
        "back" => path(vec![m(14., 6.), m(8., 12.), m(14., 18.)]),
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

// ---------------------------------------------------------------- android insets (JNI via ndk-context)

#[cfg(target_os = "android")]
fn query_insets(ppp: f32) -> (f32, f32) {
    use jni::objects::JObject;

    let read = || -> Option<(i32, i32)> {
        let cctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(cctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let act = unsafe { JObject::from_raw(cctx.context().cast()) };
        env.push_local_frame(16).ok()?;
        let mut res: Option<(i32, i32)> = None;
        let ok = (|| -> Option<()> {
            let win = env.call_method(&act, "getWindow", "()Landroid/view/Window;", &[]).ok()?.l().ok()?;
            let dv = env.call_method(&win, "getDecorView", "()Landroid/view/View;", &[]).ok()?.l().ok()?;
            let ins = env.call_method(&dv, "getRootWindowInsets", "()Landroid/view/WindowInsets;", &[]).ok()?.l().ok()?;
            let top = env.call_method(&ins, "getSystemWindowInsetTop", "()I", &[]).ok()?.i().ok()?;
            let bot = env.call_method(&ins, "getSystemWindowInsetBottom", "()I", &[]).ok()?.i().ok()?;
            res = Some((top, bot));
            Some(())
        })();
        let _ = unsafe { env.pop_local_frame(&JObject::null()) };
        ok?;
        res
    };
    match read() {
        Some((top, bot)) => (((top as f32) / ppp).max(0.), ((bot as f32) / ppp).max(0.)),
        None => (0., 0.),
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
        Pal::Pearl => "bone",
        Pal::Sky => "blue",
        Pal::Amber => "amber",
        Pal::Green => "green",
    }
}

fn load_settings(path: &Option<PathBuf>) -> (bool, Pal, f32) {
    let (mut dark, mut pal, mut vol) = (false, Pal::Pearl, 0.8f32);
    if let Some(p) = path {
        if let Ok(text) = std::fs::read_to_string(p) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "dark" => dark = v.trim() == "1",
                        "pal" => pal = match v.trim() {
                            "blue" => Pal::Sky,
                            "amber" => Pal::Amber,
                            "green" => Pal::Green,
                            _ => Pal::Pearl,
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
    drill: Option<(bool, usize)>,
    query: String,
    search: bool,
    query_new: bool,
    dark: bool,
    pal: Pal,
    panel: bool,
    player: bool,
    player_t: f32,
    vol: f32,
    seeking: Option<f32>,
    rot: f32,
    now: f64,
    dt: f32,
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
            drill: None,
            query: String::new(),
            search: false,
            query_new: false,
            dark,
            pal,
            panel: false,
            player: false,
            player_t: 0.,
            vol,
            seeking: None,
            rot: 0.,
            now: 0.,
            dt: 1. / 60.,
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
        info!("play [{}] {}", i, title);
    }

    fn toggle(&mut self) {
        if self.songs.is_empty() {
            return;
        }
        if self.sim {
            self.playing = !self.playing;
            info!("toggle (sim) -> playing={}", self.playing);
            return;
        }
        if self.loaded == Some(self.cur) {
            if let Some(a) = &self.audio {
                if self.playing { a.pause(); } else { a.resume(); }
            }
            self.playing = !self.playing;
            info!("toggle -> playing={}", self.playing);
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
        if auto && self.rep == 2 {
            self.play_index(self.cur);
            return;
        }
        let i = if self.shuf {
            let j = self.rnd(n);
            if n > 1 && j == self.cur { (j + 1) % n } else { j }
        } else {
            self.cur + 1
        };
        if i >= n {
            if auto && self.rep == 0 {
                self.stop();
                info!("queue: end reached — stopping");
                return;
            }
            self.play_index(0);
        } else {
            self.play_index(i);
        }
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
        let i = if self.shuf { self.rnd(n) } else { (self.cur + n - 1) % n };
        info!("prev -> [{i}]");
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
        info!("seek -> {t:.1}s");
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

fn sense(on: bool) -> Sense {
    if on { Sense::click() } else { Sense::hover() }
}

fn slider(ui: &mut Ui, rect: Rect, v: f32, c: C, interactive: bool) -> (f32, bool, bool) {
    let resp = ui.allocate_rect(rect, if interactive { Sense::click_and_drag() } else { Sense::hover() });
    let p = ui.painter();
    let t = rect.center().y;
    let tr = Rect::from_min_max(pos2(rect.left(), t - 2.5), pos2(rect.right(), t + 2.5));
    p.rect_filled(tr, 2.5, c.track);
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
        grad(p, fr, 2.5, &|q| mix(c.a1, c.a2, (q.x - rect.left()) / rect.width().max(1.)));
    }
    let trect = Rect::from_center_size(pos2(x, t), vec2(18., 18.));
    soft_shadow(p, trect, 9., 1.5, 5., c.shb, 0.25);
    p.circle_filled(pos2(x, t), 9., Color32::WHITE);
    p.circle_stroke(pos2(x, t), 7.2, sk(2.0, c.ac));
    (nv, resp.dragged(), resp.drag_stopped() || resp.clicked())
}

fn blobs(p: &Painter, area: Rect, now: f64, c: C) {
    let defs = [
        (area.width() * 0.85, pos2(area.right() - 40., area.top() + 70.), c.bl[0], c.blo[0]),
        (area.width() * 0.7, pos2(area.left() + 20., area.bottom() - 180.), c.bl[1], c.blo[1]),
        (area.width() * 0.6, pos2(area.right() - 30., area.top() + area.height() * 0.52), c.bl[2], c.blo[2]),
    ];
    for (k, (d, ce0, col, op)) in defs.into_iter().enumerate() {
        let u = 0.5 + 0.5 * ((now * 0.4 + k as f64 * 2.1).sin() as f32);
        let ce = ce0 + vec2(26. * u - 13., -34. * u + 17.);
        radial(p, ce, d / 2., d / 2., fade(col, op));
    }
}

fn empty_state(ui: &mut Ui, c: C) {
    let w = ui.available_width();
    let resp = ui.allocate_response(vec2(w, 170.), Sense::hover());
    let rect = resp.rect;
    let p = ui.painter();
    icon(p, "note", pos2(rect.center().x, rect.center().y - 18.), 36., fade(c.mu, 0.9));
    txt(p, pos2(rect.center().x, rect.center().y + 24.), Align2::CENTER_CENTER,
        "Looking for music on your device…", f4(13.), c.mu);
}

impl App {
    fn draw_bg(&self, p: &Painter, area: Rect, c: C) {
        quad_grad(p, area, c.bg1, c.bg2, 160.);
        blobs(p, area, self.now, c);
    }

    fn draw_header(&mut self, ui: &mut Ui, area: Rect, c: C) {
        let on = self.player_t < 0.5;
        let p = ui.painter_at(area);
        let hy = area.top();
        let ir = |k: f32| Rect::from_center_size(pos2(area.right() - 41. - k * 47., hy + 27.), vec2(38., 38.));
        let pal_r = ir(0.);
        let thm_r = ir(1.);
        let sea_r = ir(2.);
        if self.search {
            let srect = Rect::from_min_max(pos2(area.left() + 16., hy + 9.), pos2(sea_r.left() - 6., hy + 45.));
            glass(&p, srect, 17., c, 1.);
            let rx = ui.allocate_rect(pal_r, sense(on));
            icon(&p, "x", pal_r.center(), 16., c.tx);
            if on && rx.clicked() {
                self.search = false;
                self.query.clear();
                info!("search: closed");
            }
            let q = ui.put(
                srect.shrink2(vec2(12., 6.)),
                TextEdit::singleline(&mut self.query)
                    .hint_text("Search title, artist, album…")
                    .frame(false)
                    .desired_width(srect.width() - 24.)
                    .font(f6(14.5))
                    .text_color(c.tx),
            );
            if self.query_new {
                q.request_focus();
                self.query_new = false;
            }
        } else {
            txt(&p, pos2(area.left() + 22., hy + 27.), Align2::LEFT_CENTER, "Velora", f8(25.), c.tx);
            let rs = ui.allocate_rect(sea_r, sense(on));
            let rt = ui.allocate_rect(thm_r, sense(on));
            let rp = ui.allocate_rect(pal_r, sense(on));
            icon(&p, "search", sea_r.center(), 19., c.tx);
            icon(&p, "theme", thm_r.center(), 20., c.tx);
            icon(&p, "pal", pal_r.center(), 20., c.tx);
            if on && rs.clicked() {
                self.search = true;
                self.query_new = true;
                info!("search: opened");
            }
            if on && rt.clicked() {
                self.dark = !self.dark;
                save_settings(&self.cfg_path, self.dark, self.pal, self.vol);
                info!("theme: dark={}", self.dark);
            }
            if on && rp.clicked() {
                self.panel = !self.panel;
            }
        }
    }

    fn draw_tabs(&mut self, ctx: &Context, ui: &mut Ui, area: Rect, c: C) {
        let on = self.player_t < 0.5;
        let bar = Rect::from_min_max(pos2(area.left() + 16., area.top() + 58.), pos2(area.right() - 16., area.top() + 104.));
        let p = ui.painter_at(bar.expand(10.));
        glass(&p, bar, 22., c, 1.);
        let sw = bar.width() / 4.;
        let idx = self.tab as usize;
        let px = ctx.animate_value_with_time(Id::new("tabpill"), bar.left() + idx as f32 * sw, 0.2);
        accent(&p, Rect::from_min_max(pos2(px + 4., bar.top() + 4.), pos2(px + sw - 4., bar.bottom() - 4.)), 18., c);
        for (k, lab) in ["Songs", "Albums", "Artists", "Queue"].iter().enumerate() {
            let seg = Rect::from_min_max(
                pos2(bar.left() + k as f32 * sw, bar.top()),
                pos2(bar.left() + (k + 1) as f32 * sw, bar.bottom()),
            );
            let r = ui.allocate_rect(seg, sense(on));
            txt(&p, seg.center(), Align2::CENTER_CENTER, *lab,
                if k == idx { f6(14.) } else { f4(13.5) },
                if k == idx { c.acon } else { c.mu });
            if on && r.clicked() {
                let nt = match k {
                    0 => Tab::Songs,
                    1 => Tab::Albums,
                    2 => Tab::Singers,
                    _ => Tab::Playlist,
                };
                if nt != self.tab {
                    info!("tab: -> {lab}");
                    self.tab = nt;
                    self.drill = None;
                }
            }
        }
    }

    fn row(&mut self, ui: &mut Ui, i: usize, w: f32, on: bool, c: C) {
        let (title, sub) = {
            let s = &self.songs[i];
            (s.title.clone(), format!("{}  •  {}", s.artist, fmt(s.dur)))
        };
        let cov = self.covers.get(&i).map(|t| t.id());
        let is_cur = i == self.cur;
        let resp = ui.allocate_response(vec2(w, 66.), sense(on));
        let r = resp.rect;
        let p = ui.painter();
        if is_cur {
            p.rect_filled(r, 16., c.hl);
        }
        let tr = Rect::from_center_size(pos2(r.left() + 40., r.center().y), vec2(48., 48.));
        match cov {
            Some(id) => tex_round(p, id, tr, 12.),
            None => tile(p, tr, 12., c, tile_letter(&title)),
        }
        txt(p, pos2(r.left() + 74., r.center().y - 9.), Align2::LEFT_CENTER,
            ellip(p, &title, f6(14.5), w - 165.), f6(14.5), if is_cur { c.ac } else { c.tx });
        txt(p, pos2(r.left() + 74., r.center().y + 10.), Align2::LEFT_CENTER,
            ellip(p, &sub, f4(11.5), w - 165.), f4(11.5), c.mu);
        if is_cur && self.playing {
            for k in 0..3u32 {
                let yy = r.center().y - 5. * ((self.now * 5. + k as f64 * 0.9).sin() as f32).max(0.);
                p.circle_filled(pos2(r.right() - 26. + k as f32 * 7., yy), 2.2, c.ac);
            }
        }
        if on && resp.clicked() {
            if is_cur {
                self.player = true;
            } else {
                self.play_index(i);
                self.player = true;
            }
        }
    }

    fn draw_list(&mut self, ui: &mut Ui, c: C) {
        let on = self.player_t < 0.5;
        match self.tab {
            Tab::Songs | Tab::Playlist => {
                let idxs = if self.tab == Tab::Songs {
                    self.filtered()
                } else {
                    (0..self.songs.len()).collect::<Vec<_>>()
                };
                if idxs.is_empty() {
                    ScrollArea::vertical().id_salt("empty").show(ui, |ui| empty_state(ui, c));
                    return;
                }
                let total = idxs.len();
                let tab = self.tab;
                ScrollArea::vertical().id_salt(format!("list-{tab:?}")).show_rows(ui, 66., total, |ui, range| {
                    ui.spacing_mut().item_spacing = vec2(0., 6.);
                    let w = ui.available_width();
                    for r in range {
                        self.row(ui, idxs[r], w, on, c);
                    }
                });
            }
            Tab::Albums | Tab::Singers => {
                ScrollArea::vertical().id_salt("grid").show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0., 6.);
                    let w = ui.available_width();
                    if let Some((is_art, di)) = self.drill {
                        let (name, idxs) = if is_art {
                            (self.artists[di].0.clone(), self.artists[di].1.clone())
                        } else {
                            (self.albums[di].0.clone(), self.albums[di].1.clone())
                        };
                        let hdr = ui.allocate_response(vec2(w, 42.), sense(on));
                        let hr = hdr.rect;
                        let p = ui.painter();
                        icon(p, "back", pos2(hr.left() + 14., hr.center().y), 18., c.tx);
                        txt(p, pos2(hr.left() + 34., hr.center().y), Align2::LEFT_CENTER,
                            ellip(p, &name, f6(17.), w - 120.), f6(17.), c.tx);
                        txt(p, pos2(hr.right(), hr.center().y), Align2::RIGHT_CENTER,
                            format!("{} tracks", idxs.len()), f4(11.5), c.mu);
                        if on && hdr.clicked() {
                            self.drill = None;
                            info!("drill: back");
                        }
                        for i in idxs {
                            self.row(ui, i, w, on, c);
                        }
                    } else {
                        let src: &Vec<(String, Vec<usize>)> =
                            if self.tab == Tab::Albums { &self.albums } else { &self.artists };
                        let cards: Vec<(String, usize, Option<TextureId>)> = src.iter().enumerate()
                            .map(|(gi, (n, v))| (n.clone(), gi, v.first().and_then(|&i| self.covers.get(&i).map(|t| t.id()))))
                            .collect();
                        if cards.is_empty() {
                            empty_state(ui, c);
                            return;
                        }
                        let cw = (w - 10.) / 2.;
                        for chunk in cards.chunks(2) {
                            for (name, gi, cov) in chunk {
                                let resp = ui.allocate_response(vec2(cw, 158.), sense(on));
                                let r = resp.rect;
                                let p = ui.painter();
                                glass(p, r, 18., c, 1.);
                                let tr = Rect::from_center_size(pos2(r.center().x, r.top() + 50.), vec2(72., 72.));
                                match *cov {
                                    Some(id) => tex_round(p, id, tr, 16.),
                                    None => tile(p, tr, 16., c, tile_letter(name)),
                                }
                                txt(p, pos2(r.center().x, r.top() + 104.), Align2::CENTER_CENTER,
                                    ellip(p, name, f6(13.5), cw - 16.), f6(13.5), c.tx);
                                txt(p, pos2(r.center().x, r.top() + 125.), Align2::CENTER_CENTER,
                                    format!("{} tracks", if self.tab == Tab::Albums { self.albums[*gi].1.len() } else { self.artists[*gi].1.len() }),
                                    f4(11.), c.mu);
                                if on && resp.clicked() {
                                    self.drill = Some((self.tab == Tab::Singers, *gi));
                                    info!("drill: into '{name}'");
                                }
                            }
                            if chunk.len() == 1 {
                                ui.allocate_response(vec2(cw, 0.001), Sense::hover());
                            }
                        }
                    }
                    ui.allocate_response(vec2(w, 16.), Sense::hover());
                });
            }
        }
    }

    fn draw_mini(&mut self, ui: &mut Ui, mini: Rect, c: C) {
        let on = self.player_t < 0.5;
        let p = ui.painter_at(mini.expand(24.));
        glass(&p, mini, 20., c, 1.);
        let d = self.dur().max(1.);
        let fr = (self.pos() / d).clamp(0., 1.);
        let pr = Rect::from_min_max(pos2(mini.left() + 12., mini.top() + 2.), pos2(mini.right() - 12., mini.top() + 4.));
        p.rect_filled(pr, 1., c.track);
        if fr > 0.004 {
            grad(&p, Rect::from_min_max(pr.min, pos2(pr.left() + fr * pr.width(), pr.max.y)), 1.,
                &|q| mix(c.a1, c.a2, (q.x - pr.left()) / pr.width().max(1.)));
        }
        let (title, artist) = self.now_meta();
        let cov = self.cur_cover_id();
        let pb = Rect::from_center_size(pos2(mini.right() - 36., mini.center().y + 2.), vec2(46., 46.));
        let nb = Rect::from_center_size(pos2(mini.right() - 88., mini.center().y + 2.), vec2(38., 38.));
        let open_r = Rect::from_min_max(pos2(mini.left() + 4., mini.top()), pos2(nb.left() - 4., mini.bottom()));
        let ropen = ui.allocate_rect(open_r, sense(on));
        let rnb = ui.allocate_rect(nb, sense(on));
        let rpb = ui.allocate_rect(pb, sense(on));
        if on && rpb.clicked() { self.toggle(); }
        if on && rnb.clicked() { self.next(false); }
        if on && ropen.clicked() {
            self.player = true;
            info!("player: opened");
        }
        let th = Rect::from_center_size(pos2(mini.left() + 36., mini.center().y + 2.), vec2(46., 46.));
        match cov {
            Some(id) => tex_round(&p, id, th, 12.),
            None => tile(&p, th, 12., c, tile_letter(&title)),
        }
        let tw = nb.left() - th.right() - 34.;
        txt(&p, pos2(th.right() + 12., mini.center().y - 8.), Align2::LEFT_CENTER,
            ellip(&p, &title, f6(14.5), tw), f6(14.5), c.tx);
        txt(&p, pos2(th.right() + 12., mini.center().y + 11.), Align2::LEFT_CENTER,
            ellip(&p, &artist, f4(11.5), tw), f4(11.5), c.mu);
        icon(&p, "next", nb.center(), 19., c.tx);
        accent(&p, pb, 23., c);
        icon(&p, if self.playing { "pause" } else { "play" }, pb.center(), 19., c.acon);
    }

    fn draw_sheet(&mut self, ui: &mut Ui, sr: Rect, c: C) {
        let t = self.player_t;
        if t < 0.004 {
            return;
        }
        let on = t > 0.95;
        let top = sr.top() + (1. - t) * sr.height();
        let p = ui.painter_at(sr);
        let area = Rect::from_min_max(pos2(sr.left(), top), sr.max);
        quad_grad(&p, area, c.bg1, c.bg2, 160.);
        blobs(&p, area, self.now, c);
        p.rect_filled(Rect::from_min_size(pos2(sr.left(), top), vec2(sr.width(), 1.)), 0., c.gb);
        p.rect_filled(Rect::from_center_size(pos2(sr.center().x, top + 12.), vec2(46., 4.5)), 2.25, fade(c.mu, 0.7));

        let xb = Rect::from_center_size(pos2(sr.right() - 32., top + 36.), vec2(40., 40.));
        let rx = ui.allocate_rect(xb, sense(on));
        icon(&p, "x", xb.center(), 17., c.tx);
        if on && rx.clicked() {
            self.player = false;
            info!("player: closed");
        }

        let cs = (sr.width() - 96.).min(sr.height() * 0.34).min(260.).max(120.);
        let cc = pos2(sr.center().x, top + 66. + cs / 2.);
        let breathe = if self.playing { ((self.now * 2.2).sin() as f32) * 0.006 } else { 0. };
        let crect = Rect::from_center_size(cc, Vec2::splat(cs * (1. + breathe)));
        let rc = ui.allocate_rect(crect, if on { Sense::click_and_drag() } else { Sense::hover() });
        if on && rc.drag_delta().y > 60. {
            self.player = false;
            info!("player: dismissed by drag");
        }
        if on && rc.clicked() {
            self.toggle();
        }
        soft_shadow(&p, crect, 28., 12., 24., c.shb, 0.45);
        match self.cur_cover_id() {
            Some(id) => {
                tex_round(&p, id, crect, 30.);
                p.rect_stroke(crect.shrink(0.5), 29.5, sk(1.0, c.gb));
            }
            None => disc(&p, cc, cs, self.rot, c),
        }

        let d = self.dur().max(0.001);
        let pos = self.pos().clamp(0., d);
        let (title, artist) = self.now_meta();
        let ty = cc.y + cs / 2. + 26.;
        txt(&p, pos2(sr.center().x, ty), Align2::CENTER_CENTER,
            ellip(&p, &title, f8(22.), sr.width() - 64.), f8(22.), c.tx);
        txt(&p, pos2(sr.center().x, ty + 25.), Align2::CENTER_CENTER,
            ellip(&p, &artist, f4(13.5), sr.width() - 64.), f4(13.5), c.mu);

        let srect = Rect::from_min_max(pos2(sr.left() + 30., ty + 48.), pos2(sr.right() - 30., ty + 76.));
        let (nv, dragging, done) = slider(ui, srect, pos / d, c, on);
        if on {
            if dragging {
                self.seeking = Some(nv * d);
            } else if done {
                self.seek(nv * d);
            }
        }
        txt(&p, pos2(srect.left(), srect.bottom() + 13.), Align2::LEFT_CENTER,
            fmt(if dragging { nv * d } else { pos }), f4(11.), c.mu);
        txt(&p, pos2(srect.right(), srect.bottom() + 13.), Align2::RIGHT_CENTER, fmt(d), f4(11.), c.mu);

        let cy = srect.bottom() + 48.;
        let fr = |f: f32| sr.left() + sr.width() * f;
        let sb = Rect::from_center_size(pos2(fr(0.13), cy), vec2(34., 34.));
        let pvb = Rect::from_center_size(pos2(fr(0.315), cy), vec2(38., 38.));
        let pb = Rect::from_center_size(pos2(fr(0.5), cy), vec2(76., 76.));
        let nxb = Rect::from_center_size(pos2(fr(0.685), cy), vec2(38., 38.));
        let rb = Rect::from_center_size(pos2(fr(0.87), cy), vec2(34., 34.));

        let rs = ui.allocate_rect(sb, sense(on));
        icon(&p, "shuf", sb.center(), 21., if self.shuf { c.ac } else { c.mu });
        if on && rs.clicked() {
            self.shuf = !self.shuf;
            info!("shuffle={}", self.shuf);
        }
        let rp = ui.allocate_rect(pvb, sense(on));
        icon(&p, "prev", pvb.center(), 24., c.tx);
        if on && rp.clicked() { self.prev(); }
        let rpb = ui.allocate_rect(pb, sense(on));
        accent(&p, pb, 38., c);
        icon(&p, if self.playing { "pause" } else { "play" }, pb.center(), 30., c.acon);
        if on && rpb.clicked() { self.toggle(); }
        let rn = ui.allocate_rect(nxb, sense(on));
        icon(&p, "next", nxb.center(), 24., c.tx);
        if on && rn.clicked() { self.next(false); }
        let rr = ui.allocate_rect(rb, sense(on));
        icon(&p, "rep", rb.center(), 21., if self.rep != 0 { c.ac } else { c.mu });
        if self.rep == 2 {
            txt(&p, pos2(rb.right() - 1., rb.top() + 1.), Align2::RIGHT_TOP, "1", f8(9.), c.ac);
        }
        if on && rr.clicked() {
            self.rep = (self.rep + 1) % 3;
            info!("repeat mode {}", self.rep);
        }

        let vy = cy + 64.;
        icon(&p, "vol", pos2(sr.left() + 40., vy), 20., c.mu);
        let vrect = Rect::from_min_max(pos2(sr.left() + 62., vy - 14.), pos2(sr.right() - 34., vy + 14.));
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
        txt(&p, pos2(sr.center().x, vy + 27.), Align2::CENTER_CENTER,
            format!("{}%", (self.vol * 100.).round() as u32), f4(11.), c.mu);
    }

    fn draw_panel(&mut self, ctx: &Context, ui: &mut Ui, sr: Rect, area: Rect, c: C) {
        if !self.panel {
            return;
        }
        let back = ui.allocate_rect(sr, Sense::click());
        if back.clicked() {
            self.panel = false;
        }
        let p = ui.painter_at(sr);
        let cw = 250.;
        let card = Rect::from_min_size(pos2(sr.right() - 20. - cw, area.top() + 56.), vec2(cw, 150.));
        soft_shadow(&p, card, 20., 6., 16., c.shb, c.sh_a);
        grad(&p, card, 20., &|_| c.sheet);
        p.rect_stroke(card.shrink(0.5), 19.5, sk(1.0, c.gb));
        txt(&p, pos2(card.left() + 18., card.top() + 26.), Align2::LEFT_CENTER, "Appearance", f8(15.), c.tx);

        let tog = Rect::from_center_size(pos2(card.right() - 42., card.top() + 60.), vec2(46., 26.));
        let rt = ui.allocate_rect(tog, Sense::click());
        p.rect_filled(tog, 13., if self.dark { c.ac } else { c.track });
        let kx = ctx.animate_value_with_time(Id::new("darkknob"),
            if self.dark { tog.right() - 14. } else { tog.left() + 14. }, 0.15);
        p.circle_filled(pos2(kx, tog.center().y), 10., Color32::WHITE);
        txt(&p, pos2(card.left() + 18., tog.center().y), Align2::LEFT_CENTER, "Dark mode", f6(14.), c.tx);
        if rt.clicked() {
            self.dark = !self.dark;
            save_settings(&self.cfg_path, self.dark, self.pal, self.vol);
            info!("theme: dark={}", self.dark);
        }

        txt(&p, pos2(card.left() + 18., card.top() + 104.), Align2::LEFT_CENTER, "Accent", f4(12.), c.mu);
        for (k, pl) in [Pal::Pearl, Pal::Sky, Pal::Amber, Pal::Green].into_iter().enumerate() {
            let ce = pos2(card.left() + 34. + k as f32 * ((cw - 68.) / 3.), card.top() + 132.);
            let rr = ui.allocate_rect(Rect::from_center_size(ce, vec2(36., 36.)), Sense::click());
            p.circle_filled(ce, 13., theme(self.dark, pl).k2);
            if pl == self.pal {
                p.circle_stroke(ce, 16.5, sk(2.0, c.ac));
            }
            if rr.clicked() {
                self.pal = pl;
                save_settings(&self.cfg_path, self.dark, self.pal, self.vol);
                info!("palette: -> {pl:?}");
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.now = ctx.input(|i| i.time);
        self.dt = ctx.input(|i| i.stable_dt).max(1. / 120.);
        self.player_t = ctx.animate_value_with_time(Id::new("sheet"), if self.player { 1. } else { 0. }, 0.28);

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
                let ni = query_insets(ctx.pixels_per_point());
                if (ni.0 - self.insets.0).abs() > 0.5 || (ni.1 - self.insets.1).abs() > 0.5 {
                    info!("insets: top={:.0} bottom={:.0}", ni.0, ni.1);
                    self.insets = ni;
                }
            }
        }

        self.tick();
        if self.playing {
            self.rot = (self.rot + self.dt * 40.) % 360.;
        }

        let c = theme(self.dark, self.pal);

        CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let sr = ui.max_rect();
            let area = Rect::from_min_max(
                pos2(sr.left(), sr.top() + self.insets.0),
                pos2(sr.right(), sr.bottom() - self.insets.1),
            );
            let bg = ui.painter_at(area.expand(2.));
            self.draw_bg(&bg, area, c);
            let mini = Rect::from_min_max(pos2(area.left() + 12., area.bottom() - 74.), pos2(area.right() - 12., area.bottom() - 10.));
            self.draw_header(ui, area, c);
            self.draw_tabs(ctx, ui, area, c);
            self.draw_list(ui, c);
            if self.player_t < 0.98 {
                self.draw_mini(ui, mini, c);
            }
            self.draw_sheet(ui, sr, c);
            self.draw_panel(ctx, ui, sr, area, c);
        });

        ctx.request_repaint_after(Duration::from_millis(33));
    }
}
