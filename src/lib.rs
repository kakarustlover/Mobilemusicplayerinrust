//! Velora - a glass-style music player written entirely in Rust.
//! UI: egui/eframe - audio: rodio + symphonia - tags: lofty - Android glue: android-activity + jni.

use eframe::egui::{self, *};
use lofty::prelude::*;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::collections::BTreeMap;
use std::f32::consts::TAU;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use winit::platform::android::activity::AndroidApp;

const APP_NAME: &str = "Velora";
const PROFILE_NAME: &str = "Kian";

// ---------------------------------------------------------------- data

#[derive(Clone)]
struct Song {
    path: PathBuf,
    title: String,
    artist: String,
    album: String,
    dur: f32,
    rot: f32,
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
    Sky,
    Amber,
    Pearl,
}

enum Act {
    Play(usize),
    Filter(String),
}

// ---------------------------------------------------------------- colors

#[derive(Clone, Copy)]
struct C {
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
    acon: Color32,
    glow: Color32,
    shadow: Color32,
    border: Color32,
    card: Color32,
    sheet: Color32,
    g1: f32,
    g2: f32,
    gl: f32,
    bo: f32,
}

fn hx(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

fn rgba(h: u32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied((h >> 16) as u8, (h >> 8) as u8, h as u8, (a.clamp(0., 1.) * 255.) as u8)
}

fn fade(c: Color32, a: f32) -> Color32 {
    let [r, g, b, al] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (al as f32 * a.clamp(0., 1.)) as u8)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0., 1.);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn ease(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

fn theme(dark: bool, pal: Pal) -> C {
    let (a1, a2, b1, b2, b3, k1, k2, acon, glow) = match pal {
        Pal::Sky => (0x7dd3fc, 0x2563eb, 0x7dd3fc, 0xbfdbfe, 0xe0f2fe, 0x2d70f0, 0x1e40af, 0xffffff, 0x2563eb),
        Pal::Amber => (0xfdba74, 0xea580c, 0xfdba74, 0xfed7aa, 0xfff1e6, 0xffb066, 0xf97316, 0x2b1204, 0xea580c),
        Pal::Pearl => (0xeceff3, 0x94a3b8, 0xd9d3c4, 0xe9e4d8, 0xf7f3ea, 0xf6f8fb, 0xa9b4c4, 0x1e293b, 0x64748b),
    };
    let (bg1, bg2, sheet) = match (dark, pal) {
        (false, Pal::Sky) => (0xe4f3ff, 0xffffff, rgba(0xffffff, 0.88)),
        (false, Pal::Amber) => (0xfff1e3, 0xffffff, rgba(0xffffff, 0.88)),
        (false, Pal::Pearl) => (0xf1ebdf, 0xfbfaf6, rgba(0xffffff, 0.88)),
        (true, Pal::Sky) => (0x0a1322, 0x0e1b33, rgba(0x0e1b33, 0.9)),
        (true, Pal::Amber) => (0x1c1009, 0x27170b, rgba(0x27170b, 0.92)),
        (true, Pal::Pearl) => (0x141416, 0x1e1e22, rgba(0x1e1e22, 0.92)),
    };
    C {
        bg1: hx(bg1),
        bg2: hx(bg2),
        tx: hx(if dark { 0xf1f5f9 } else { 0x0f172a }),
        mu: hx(if dark { 0xa3b3c8 } else { 0x475569 }),
        a1: hx(a1),
        a2: hx(a2),
        b1: hx(b1),
        b2: hx(b2),
        b3: hx(b3),
        k1: hx(k1),
        k2: hx(k2),
        acon: hx(acon),
        glow: rgba(glow, 0.42),
        shadow: if dark { rgba(0x000000, 0.5) } else { rgba(0x0f172a, 0.14) },
        border: rgba(0xffffff, if dark { 0.3 } else { 0.9 }),
        card: rgba(0xffffff, if dark { 0.09 } else { 0.6 }),
        sheet,
        g1: if dark { 0.17 } else { 0.74 },
        g2: if dark { 0.05 } else { 0.26 },
        gl: if dark { 0.16 } else { 0.5 },
        bo: if dark { 0.4 } else { 0.6 },
    }
}

// ---------------------------------------------------------------- drawing helpers

fn round_pts(rect: Rect, r: f32) -> Vec<Pos2> {
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
            let a = (a0 + 90.0 * k as f32 / 8.0).to_radians();
            v.push(pos2(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    v
}

/// Rounded shape filled with a per-vertex color function (real gradients).
fn fill(p: &Painter, rect: Rect, r: f32, f: impl Fn(Pos2) -> Color32) {
    let pts = round_pts(rect, r);
    let mut m = Mesh::default();
    m.colored_vertex(rect.center(), f(rect.center()));
    for q in &pts {
        m.colored_vertex(*q, f(*q));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

fn glass(p: &Painter, rect: Rect, r: f32, c: &C, a: f32) {
    p.add(Shadow { offset: vec2(0., 8.), blur: 20., spread: 0., color: fade(c.shadow, a) }.as_shape(rect, r));
    fill(p, rect, r, |q| {
        let t = ((q.x - rect.left()) / rect.width() + (q.y - rect.top()) / rect.height()) * 0.5;
        let y = (q.y - rect.top()) / rect.height();
        let base = c.g1 + (c.g2 - c.g1) * t;
        let gloss = ((0.52 - y) / 0.52).clamp(0., 1.) * c.gl * 0.7;
        Color32::from_rgba_unmultiplied(255, 255, 255, ((base + gloss).min(1.) * a * 255.) as u8)
    });
    p.rect_stroke(rect, r, Stroke::new(1., fade(c.border, a)));
}

fn accent(p: &Painter, rect: Rect, r: f32, c: &C, a: f32) {
    p.add(Shadow { offset: vec2(0., 8.), blur: 20., spread: 0., color: fade(c.glow, a) }.as_shape(rect, r));
    fill(p, rect, r, |q| {
        let t = ((q.x - rect.left()) / rect.width() + (q.y - rect.top()) / rect.height()) * 0.5;
        let y = (q.y - rect.top()) / rect.height();
        let g = ((0.52 - y) / 0.52).clamp(0., 1.) * 0.5;
        fade(mix(mix(c.k1, c.k2, t), Color32::WHITE, g), a)
    });
    p.rect_stroke(rect, r, Stroke::new(1., fade(Color32::WHITE, a * 0.6)));
}

fn sheet_box(p: &Painter, rect: Rect, r: impl Into<Rounding> + Copy, c: &C) {
    p.add(Shadow { offset: vec2(0., 8.), blur: 24., spread: 0., color: c.shadow }.as_shape(rect, r));
    p.rect_filled(rect, r, c.sheet);
    p.rect_stroke(rect, r, Stroke::new(1., c.border));
}

fn glow(p: &Painter, ce: Pos2, r: f32, col: Color32, a: f32) {
    let n = 14;
    let per = 1. - (1. - a.clamp(0., 0.95)).powf(1. / n as f32);
    for k in 0..n {
        let f = k as f32 / n as f32;
        p.circle_filled(ce, r * (1. - f * 0.9), fade(col, per));
    }
}

fn bg(p: &Painter, rect: Rect, c: &C, now: f64, a: f32) {
    let mut m = Mesh::default();
    m.colored_vertex(rect.left_top(), fade(c.bg1, a));
    m.colored_vertex(rect.right_top(), fade(mix(c.bg1, c.bg2, 0.35), a));
    m.colored_vertex(rect.right_bottom(), fade(c.bg2, a));
    m.colored_vertex(rect.left_bottom(), fade(mix(c.bg1, c.bg2, 0.65), a));
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(Shape::mesh(m));
    let t = now as f32;
    glow(p, pos2(rect.right() - 10. + (t * 0.4).sin() * 20., rect.top() + 40. + (t * 0.3).cos() * 20.), 200., c.b1, c.bo * a);
    glow(p, pos2(rect.left() + 10. + (t * 0.35).cos() * 20., rect.bottom() - 160. + (t * 0.3).sin() * 24.), 170., c.a2, c.bo * 0.5 * a);
    glow(p, pos2(rect.right() - 10., rect.center().y + (t * 0.25).sin() * 30.), 130., c.b2, c.bo * 0.8 * a);
}

fn disc(p: &Painter, ce: Pos2, r: f32, rot: f32, c: &C, a: f32) {
    let n = 72usize;
    let stops = [c.a1, c.a2, c.b3, c.a1];
    let mut m = Mesh::default();
    m.colored_vertex(ce, fade(c.bg2, a));
    for k in 0..=n {
        let t = k as f32 / n as f32;
        let an = rot + TAU * t;
        let s = t * 3.;
        let i = (s as usize).min(2);
        m.colored_vertex(ce + vec2(an.cos(), an.sin()) * r, fade(mix(stops[i], stops[i + 1], s - i as f32), a));
    }
    for k in 0..n as u32 {
        m.add_triangle(0, 1 + k, 2 + k);
    }
    p.add(Shape::mesh(m));
    let mut rr = r * 0.4;
    while rr < r {
        p.circle_stroke(ce, rr, Stroke::new(1., fade(Color32::WHITE, a * 0.16)));
        rr += r * 0.07;
    }
    p.circle_stroke(ce, r, Stroke::new(1., fade(Color32::WHITE, a * 0.5)));
    p.circle_filled(ce, r * 0.28, fade(c.bg2, a));
    p.circle_stroke(ce, r * 0.28, Stroke::new(2.5, fade(Color32::WHITE, a * 0.8)));
}

fn art_sq(p: &Painter, rect: Rect, r: f32, rot: f32, c: &C) {
    let d = vec2(rot.to_radians().cos(), rot.to_radians().sin());
    let cen = rect.center();
    let half = rect.width() * 0.7;
    fill(p, rect, r, |q| {
        let t = ((q - cen).dot(d) / half + 0.5).clamp(0., 1.);
        if t < 0.5 { mix(c.a1, c.a2, t * 2.) } else { mix(c.a2, c.b3, (t - 0.5) * 2.) }
    });
}

fn avatar(p: &Painter, rect: Rect, c: &C) {
    accent(p, rect, rect.width() / 2., c, 1.);
    let ch = PROFILE_NAME.chars().next().unwrap_or('V').to_string();
    p.text(rect.center(), Align2::CENTER_CENTER, ch, FontId::proportional(18.), c.acon);
}

fn slider(p: &Painter, line: Rect, f: f32, c: &C) {
    p.rect_filled(line, 3., rgba(0x808080, 0.28));
    let fr = Rect::from_min_max(line.min, pos2(line.left() + f * line.width(), line.bottom()));
    fill(p, fr, 3., |q| mix(c.a1, c.a2, (q.x - line.left()) / line.width()));
    let tp = pos2(line.left() + f * line.width(), line.center().y);
    p.circle_filled(tp, 10., Color32::WHITE);
    p.circle_stroke(tp, 10., Stroke::new(2., c.k2));
}

fn icon(p: &Painter, k: &str, ce: Pos2, s: f32, col: Color32) {
    let u = s / 24.;
    let o = ce - vec2(12., 12.) * u;
    let pt = |x: f32, y: f32| pos2(o.x + x * u, o.y + y * u);
    let st = Stroke::new(2. * u.max(0.8), col);
    let line = |v: &[(f32, f32)]| {
        p.add(Shape::line(v.iter().map(|&(x, y)| pt(x, y)).collect(), st));
    };
    let poly = |v: &[(f32, f32)]| {
        p.add(Shape::convex_polygon(v.iter().map(|&(x, y)| pt(x, y)).collect(), col, Stroke::NONE));
    };
    match k {
        "play" => poly(&[(8., 5.), (8., 19.), (19., 12.)]),
        "pause" => {
            poly(&[(6., 5.), (10., 5.), (10., 19.), (6., 19.)]);
            poly(&[(14., 5.), (18., 5.), (18., 19.), (14., 19.)]);
        }
        "prev" => {
            poly(&[(6., 6.), (8., 6.), (8., 18.), (6., 18.)]);
            poly(&[(9.5, 12.), (18., 18.), (18., 6.)]);
        }
        "next" => {
            poly(&[(16., 6.), (18., 6.), (18., 18.), (16., 18.)]);
            poly(&[(14.5, 12.), (6., 18.), (6., 6.)]);
        }
        "search" => {
            p.circle_stroke(pt(11., 11.), 7. * u, st);
            line(&[(16., 16.), (20., 20.)]);
        }
        "pal" => {
            p.circle_stroke(pt(12., 12.), 9. * u, st);
            for (x, y) in [(8., 11.), (12., 7.5), (16., 11.)] {
                p.circle_filled(pt(x, y), 1.4 * u, col);
            }
        }
        "x" => {
            line(&[(6., 6.), (18., 18.)]);
            line(&[(18., 6.), (6., 18.)]);
        }
        "down" => line(&[(6., 9.), (12., 15.), (18., 9.)]),
        "shuf" => {
            line(&[(4., 20.), (21., 3.)]);
            line(&[(4., 4.), (9., 9.)]);
            line(&[(15., 15.), (21., 21.)]);
            line(&[(16., 3.), (21., 3.), (21., 8.)]);
            line(&[(21., 16.), (21., 21.), (16., 21.)]);
        }
        "rep" => {
            line(&[(3., 11.), (3., 9.), (6., 6.), (21., 6.)]);
            line(&[(17., 2.), (21., 6.), (17., 10.)]);
            line(&[(21., 13.), (21., 15.), (18., 18.), (3., 18.)]);
            line(&[(7., 14.), (3., 18.), (7., 22.)]);
        }
        "vol" => {
            poly(&[(4., 9.), (8., 9.), (8., 15.), (4., 15.)]);
            poly(&[(8., 9.), (13., 5.), (13., 19.), (8., 15.)]);
            line(&[(16.5, 9.), (18., 12.), (16.5, 15.)]);
        }
        _ => {}
    }
}

fn fmt(s: f32) -> String {
    let s = s.max(0.) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

fn row(p: &Painter, r: Rect, c: &C, cur: bool, rot: f32, title: &str, sub: &str, right: &str) {
    if cur {
        p.rect_filled(r.shrink2(vec2(8., 2.)), 18., c.card);
    }
    let art = Rect::from_min_size(pos2(r.left() + 18., r.center().y - 20.), vec2(40., 40.));
    art_sq(p, art, 13., rot, c);
    let tx = art.right() + 12.;
    let cl = p.with_clip_rect(Rect::from_min_max(pos2(tx, r.top()), pos2(r.right() - 64., r.bottom())));
    cl.text(pos2(tx, r.center().y - 2.), Align2::LEFT_BOTTOM, title, FontId::proportional(15.), c.tx);
    cl.text(pos2(tx, r.center().y + 2.), Align2::LEFT_TOP, sub, FontId::proportional(13.), c.mu);
    p.text(pos2(r.right() - 18., r.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(13.), c.mu);
}

fn empty_box(ui: &mut Ui, c: &C, w: f32, head: &str, sub: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 120.), Sense::hover());
    let p = ui.painter();
    p.text(pos2(r.center().x, r.center().y - 12.), Align2::CENTER_CENTER, head, FontId::proportional(18.), c.tx);
    p.text(pos2(r.center().x, r.center().y + 14.), Align2::CENTER_CENTER, sub, FontId::proportional(14.), c.mu);
}

// ---------------------------------------------------------------- library scan (tags via lofty)

fn read_meta(p: &Path) -> Song {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let (mut t, mut a, mut al, mut d) = (stem, "Unknown".to_string(), "Unknown".to_string(), 0.0f32);
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
        }
    }
    let rot = (t.bytes().map(|b| b as u32).sum::<u32>() % 360) as f32;
    Song { path: p.to_path_buf(), title: t, artist: a, album: al, dur: d, rot }
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

// ---------------------------------------------------------------- audio (rodio)

struct Audio {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    sink: Option<Sink>,
}

impl Audio {
    fn new() -> Option<Self> {
        let (s, h) = OutputStream::try_default().ok()?;
        Some(Self { _stream: s, handle: h, sink: None })
    }
    fn load(&mut self, p: &Path, vol: f32) -> bool {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        let Ok(f) = File::open(p) else { return false };
        let Ok(src) = Decoder::new(BufReader::new(f)) else { return false };
        let Ok(sink) = Sink::try_new(&self.handle) else { return false };
        sink.set_volume(vol);
        sink.append(src);
        self.sink = Some(sink);
        true
    }
    fn pos(&self) -> f32 {
        self.sink.as_ref().map_or(0., |s| s.get_pos().as_secs_f32())
    }
    fn done(&self) -> bool {
        self.sink.as_ref().map_or(false, |s| s.empty())
    }
}

// ---------------------------------------------------------------- app

struct App {
    app: AndroidApp,
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
    fx_until: f64,
    now: f64,
    t0: Option<f64>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, app: AndroidApp) -> Self {
        Self {
            app,
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
            dark: cc.egui_ctx.style().visuals.dark_mode,
            pal: Pal::Sky,
            panel: false,
            player: false,
            vol: 0.8,
            seeking: None,
            rot: 0.,
            fx_until: 0.,
            now: 0.,
            t0: None,
        }
    }

    fn play_index(&mut self, i: usize) {
        let Some(s) = self.songs.get(i) else { return };
        self.cur = i;
        self.seeking = None;
        let ok = match self.audio.as_mut() {
            Some(a) => a.load(&s.path, self.vol),
            None => false,
        };
        self.loaded = if ok { Some(i) } else { None };
        self.playing = ok;
        if ok {
            self.fx_until = self.now + 5.0;
        }
    }

    fn toggle(&mut self) {
        if self.loaded == Some(self.cur) {
            if let Some(s) = self.audio.as_ref().and_then(|a| a.sink.as_ref()) {
                if self.playing { s.pause() } else { s.play() }
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
        if let Some(s) = self.audio.as_mut().and_then(|a| a.sink.take()) {
            s.stop();
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
            if let Some(sk) = self.audio.as_ref().and_then(|a| a.sink.as_ref()) {
                let _ = sk.try_seek(Duration::from_secs_f32(s.max(0.)));
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

    fn draw(&mut self, ui: &mut Ui, c: &C, now: f64, st: f32) {
        let sr = ui.max_rect();
        let p = ui.painter().clone();
        let ep = ease(ui.ctx().animate_bool_with_time(Id::new("panel"), self.panel, 0.3));
        let el = ease(ui.ctx().animate_bool_with_time(Id::new("player"), self.player, 0.4));
        let on = st > 2.6 && ep < 0.01 && el < 0.01 && !self.panel && !self.player;
        bg(&p, sr, c, now, 1.);
        self.home(ui, &p, sr, c, on);
        if ep > 0.001 {
            self.panel_ui(ui, &p, sr, c, ep);
        }
        if el > 0.001 {
            self.player_ui(ui, &p, sr, c, el, now);
        }
        if st < 2.7 {
            self.splash(&p, sr, c, now, st);
        }
    }

    fn home(&mut self, ui: &mut Ui, p: &Painter, sr: Rect, c: &C, on: bool) {
        let w = sr.width().min(460.);
        let x0 = sr.center().x - w / 2.;
        let x1 = x0 + w;
        let top = sr.top() + 12.;
        let f = |s: f32| FontId::proportional(s);

        // profile + app name
        avatar(p, Rect::from_min_size(pos2(x0 + 16., top), vec2(38., 38.)), c);
        p.text(pos2(x0 + 64., top + 2.), Align2::LEFT_TOP, "Welcome back", f(12.), c.mu);
        p.text(pos2(x0 + 64., top + 18.), Align2::LEFT_TOP, PROFILE_NAME, f(17.), c.tx);
        p.text(pos2(x1 - 16., top + 10.), Align2::RIGHT_TOP, APP_NAME, f(18.), c.tx);

        // search bar + theme button (button on the right)
        let sy = top + 38. + 14.;
        let sb = Rect::from_min_max(pos2(x0 + 16., sy), pos2(x1 - 16. - 42. - 10., sy + 46.));
        glass(p, sb, 23., c, 1.);
        icon(p, "search", pos2(sb.left() + 24., sb.center().y), 22., c.mu);
        let te = Rect::from_min_max(pos2(sb.left() + 46., sb.top()), pos2(sb.right() - 12., sb.bottom()));
        ui.allocate_ui_at_rect(te, |ui| {
            ui.set_enabled(on);
            let r = ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text(RichText::new("Search your music").color(c.mu))
                    .text_color(c.tx)
                    .font(FontId::proportional(16.))
                    .frame(false)
                    .vertical_align(Align::Center)
                    .desired_width(te.width())
                    .min_size(vec2(te.width(), 46.)),
            );
            if r.gained_focus() {
                self.app.show_soft_input(true);
            }
            if r.lost_focus() {
                self.app.hide_soft_input(false);
            }
        });
        let tb = Rect::from_min_size(pos2(x1 - 16. - 42., sy + 2.), vec2(42., 42.));
        glass(p, tb, 21., c, 1.);
        icon(p, "pal", tb.center(), 22., c.tx);
        if on && ui.interact(tb, Id::new("theme"), Sense::click()).clicked() {
            self.panel = true;
        }

        // tabs (text only)
        let ty = sy + 46. + 12.;
        let mut x = x0 + 16.;
        for (i, (label, tab)) in [("Songs", Tab::Songs), ("Albums", Tab::Albums), ("Singers", Tab::Singers), ("Playlist", Tab::Playlist)].iter().enumerate() {
            let wt = p.layout_no_wrap(label.to_string(), f(14.), c.mu).size().x + 28.;
            let r = Rect::from_min_size(pos2(x, ty), vec2(wt, 38.));
            p.text(r.center(), Align2::CENTER_CENTER, label, f(14.), if self.tab == *tab { c.tx } else { c.mu });
            if on && ui.interact(r, Id::new(("tab", i)), Sense::click()).clicked() {
                self.tab = *tab;
            }
            x += wt + 8.;
        }

        // count + list
        let ly = ty + 38. + 30.;
        if self.tab == Tab::Songs {
            let n = self.filtered().len();
            p.text(pos2(x0 + 20., ty + 38. + 8.), Align2::LEFT_TOP, format!("{} {}", n, if n == 1 { "song" } else { "songs" }), f(14.), c.mu);
        }
        let lr = Rect::from_min_max(pos2(x0, ly), pos2(x1, sr.bottom()));
        ui.allocate_ui_at_rect(lr, |ui| {
            ScrollArea::vertical().id_source("songs").auto_shrink([false, false]).show(ui, |ui| {
                self.list(ui, c, w, on);
                ui.add_space(96.);
            });
        });

        // mini player bar
        let mb = Rect::from_min_size(pos2(x0 + 14., sr.bottom() - 14. - 60.), vec2(w - 28., 60.));
        sheet_box(p, mb, 30., c);
        let (title, artist, rot) = self.songs.get(self.cur).map_or(("No music yet".to_string(), String::new(), 0.), |s| (s.title.clone(), s.artist.clone(), s.rot));
        let art = Rect::from_center_size(pos2(mb.left() + 10. + 21., mb.center().y), vec2(42., 42.));
        art_sq(p, art, 21., rot + if self.playing { self.rot.to_degrees() } else { 0. }, c);
        let nx = Rect::from_center_size(pos2(mb.right() - 10. - 20., mb.center().y), vec2(40., 40.));
        let pl = Rect::from_center_size(pos2(nx.left() - 8. - 20., mb.center().y), vec2(40., 40.));
        let tx = art.right() + 12.;
        let cl = p.with_clip_rect(Rect::from_min_max(pos2(tx, mb.top()), pos2(pl.left() - 8., mb.bottom())));
        cl.text(pos2(tx, mb.center().y - 1.), Align2::LEFT_BOTTOM, title, f(15.), c.tx);
        cl.text(pos2(tx, mb.center().y + 2.), Align2::LEFT_TOP, artist, f(13.), c.mu);
        accent(p, pl, 20., c, 1.);
        icon(p, if self.playing { "pause" } else { "play" }, pl.center(), 20., c.acon);
        icon(p, "next", nx.center(), 22., c.tx);
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

    fn list(&mut self, ui: &mut Ui, c: &C, w: f32, on: bool) {
        let p = ui.painter().clone();
        let sense = if on { Sense::click() } else { Sense::hover() };
        let mut act = None;
        match self.tab {
            Tab::Songs => {
                let idx = self.filtered();
                if idx.is_empty() {
                    if self.query.is_empty() {
                        empty_box(ui, c, w, "No songs yet", "Music stored on your phone will appear here.");
                    } else {
                        empty_box(ui, c, w, "No songs found", "Try a different search.");
                    }
                }
                for i in idx {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 56.), sense);
                    let s = &self.songs[i];
                    row(&p, r, c, i == self.cur, s.rot, &s.title, &s.artist, &fmt(s.dur));
                    if resp.clicked() {
                        act = Some(Act::Play(i));
                    }
                }
            }
            Tab::Albums | Tab::Singers => {
                let mut m: BTreeMap<String, usize> = BTreeMap::new();
                for s in &self.songs {
                    *m.entry(if self.tab == Tab::Albums { s.album.clone() } else { s.artist.clone() }).or_default() += 1;
                }
                if m.is_empty() {
                    empty_box(ui, c, w, "Nothing here yet", "Your music library is empty.");
                }
                for (k, (name, n)) in m.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 56.), sense);
                    row(&p, r, c, false, k as f32 * 72. + 30., name, &format!("{} {}", n, if *n == 1 { "song" } else { "songs" }), "");
                    if resp.clicked() {
                        act = Some(Act::Filter(name.clone()));
                    }
                }
            }
            Tab::Playlist => empty_box(ui, c, w, "No playlists yet", "Playlists you create will appear here."),
        }
        match act {
            Some(Act::Play(i)) => self.play_index(i),
            Some(Act::Filter(n)) => {
                self.query = n;
                self.tab = Tab::Songs;
            }
            None => {}
        }
    }

    fn panel_ui(&mut self, ui: &mut Ui, p: &Painter, sr: Rect, c: &C, e: f32) {
        p.rect_filled(sr, 0., Color32::from_rgba_unmultiplied(10, 18, 34, (0.38 * e * 255.) as u8));
        if self.panel && ui.interact(sr, Id::new("pn_back"), Sense::click()).clicked() {
            self.panel = false;
        }
        let w = sr.width().min(460.);
        let x0 = sr.center().x - w / 2.;
        let x1 = x0 + w;
        let h = 290.;
        let pr = Rect::from_min_size(pos2(x0, sr.top() - h * (1. - e)), vec2(w, h));
        sheet_box(p, pr, Rounding { nw: 0., ne: 0., sw: 32., se: 32. }, c);
        ui.interact(pr, Id::new("pn_bg"), Sense::click());
        let f = |s: f32| FontId::proportional(s);
        p.text(pos2(x0 + 20., pr.top() + 30.), Align2::LEFT_CENTER, "Appearance", f(19.), c.tx);
        let xr = Rect::from_center_size(pos2(x1 - 41., pr.top() + 30.), vec2(42., 42.));
        icon(p, "x", xr.center(), 22., c.tx);
        if ui.interact(xr, Id::new("pn_x"), Sense::click()).clicked() {
            self.panel = false;
        }
        p.text(pos2(x0 + 20., pr.top() + 72.), Align2::LEFT_CENTER, "App color", f(14.), c.mu);
        let cw = (w - 40. - 20.) / 3.;
        for (i, (pl, name, l, d)) in [(Pal::Sky, "Blue & White", 0xbae6fd, 0x2563eb), (Pal::Amber, "Orange & White", 0xfed7aa, 0xea580c), (Pal::Pearl, "Bone & Silver", 0xf7f3ea, 0x94a3b8)].iter().enumerate() {
            let cell = Rect::from_min_size(pos2(x0 + 20. + i as f32 * (cw + 10.), pr.top() + 88.), vec2(cw, 84.));
            let sw = Rect::from_center_size(pos2(cell.center().x, cell.top() + 27.), vec2(38., 38.));
            fill(p, sw, 19., |q| mix(hx(*l), hx(*d), ((q.x - sw.left()) + (q.y - sw.top())) / (2. * sw.width())));
            p.circle_stroke(sw.center(), 19., Stroke::new(2., Color32::WHITE));
            if self.pal == *pl {
                let cc = sw.center();
                let pts = vec![cc + vec2(-7., 0.), cc + vec2(-2., 5.), cc + vec2(7., -5.)];
                p.add(Shape::line(pts.clone(), Stroke::new(4.5, rgba(0x000000, 0.35))));
                p.add(Shape::line(pts, Stroke::new(3., Color32::WHITE)));
            }
            p.text(pos2(cell.center().x, cell.top() + 64.), Align2::CENTER_CENTER, *name, f(13.), c.tx);
            if ui.interact(cell, Id::new(("pal", i)), Sense::click()).clicked() {
                self.pal = *pl;
            }
        }
        p.text(pos2(x0 + 20., pr.top() + 196.), Align2::LEFT_CENTER, "Display mode", f(14.), c.mu);
        let sw = (w - 40. - 10.) / 2.;
        for (i, (name, d)) in [("Light", false), ("Dark", true)].iter().enumerate() {
            let r = Rect::from_min_size(pos2(x0 + 20. + i as f32 * (sw + 10.), pr.top() + 216.), vec2(sw, 44.));
            let sel = self.dark == *d;
            if sel {
                accent(p, r, 22., c, 1.);
            } else {
                p.rect_filled(r, 22., c.card);
            }
            p.text(r.center(), Align2::CENTER_CENTER, *name, f(15.), if sel { c.acon } else { c.tx });
            if ui.interact(r, Id::new(("mode", i)), Sense::click()).clicked() {
                self.dark = *d;
            }
        }
    }

    fn player_ui(&mut self, ui: &mut Ui, p: &Painter, sr: Rect, c: &C, e: f32, now: f64) {
        let pr = sr.translate(vec2(0., (1. - e) * sr.height()));
        bg(p, pr, c, now, 1.);
        ui.interact(pr, Id::new("pl_bg"), Sense::click());
        let w = pr.width().min(460.);
        let x0 = pr.center().x - w / 2.;
        let x1 = x0 + w;
        let ph = pr.height();
        let sz = (w * 0.66).min(ph * 0.34).min(280.);
        let hs = [42., sz, 58., 50., 74., 36.];
        let pad = 12.;
        let gap = ((ph - hs.iter().sum::<f32>() - 2. * pad) / 5.).clamp(8., 36.);
        let f = |s: f32| FontId::proportional(s);
        let cx = pr.center().x;
        let mut y = pr.top() + pad;

        // top bar
        let cl = Rect::from_min_size(pos2(x0 + 22., y), vec2(42., 42.));
        glass(p, cl, 21., c, 1.);
        icon(p, "down", cl.center(), 22., c.tx);
        p.text(pos2(cx, y + 21.), Align2::CENTER_CENTER, "Now playing", f(14.), c.mu);
        if ui.interact(cl, Id::new("pl_close"), Sense::click()).clicked() {
            self.player = false;
        }
        y += 42. + gap;

        // art
        let art = Rect::from_center_size(pos2(cx, y + sz / 2.), vec2(sz, sz));
        glass(p, art, 40., c, 1.);
        disc(p, art.center(), sz * 0.41, self.rot, c, 1.);
        y += sz + gap;

        // title
        let (title, artist, dur) = self.songs.get(self.cur).map_or(("No music found".to_string(), "Allow audio access to scan your phone".to_string(), 1.), |s| (s.title.clone(), s.artist.clone(), s.dur.max(1.)));
        let tc = p.with_clip_rect(Rect::from_min_max(pos2(x0 + 20., y), pos2(x1 - 20., y + 58.)));
        tc.text(pos2(cx, y + 16.), Align2::CENTER_CENTER, title, f(23.), c.tx);
        tc.text(pos2(cx, y + 44.), Align2::CENTER_CENTER, artist, f(15.), c.mu);
        y += 58. + gap;

        // seek bar
        let tr = Rect::from_min_max(pos2(x0 + 22., y), pos2(x1 - 22., y + 36.));
        let line = Rect::from_center_size(pos2(tr.center().x, y + 14.), vec2(tr.width(), 6.));
        let shown = self.seeking.unwrap_or_else(|| self.pos());
        slider(p, line, (shown / dur).clamp(0., 1.), c);
        p.text(pos2(tr.left(), y + 40.), Align2::LEFT_CENTER, fmt(shown), f(13.), c.mu);
        p.text(pos2(tr.right(), y + 40.), Align2::RIGHT_CENTER, fmt(dur), f(13.), c.mu);
        let resp = ui.interact(tr, Id::new("seek"), Sense::click_and_drag());
        if let Some(pp) = resp.interact_pointer_pos() {
            let fr = ((pp.x - line.left()) / line.width()).clamp(0., 1.);
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
        y += 50. + gap;

        // controls
        let ws = [42., 52., 74., 52., 42.];
        let sp = ((w - 44.) - ws.iter().sum::<f32>()) / 4.;
        let cy = y + 37.;
        let mut xx = x0 + 22.;
        let mut rs = [Rect::NOTHING; 5];
        for (i, wd) in ws.iter().enumerate() {
            rs[i] = Rect::from_center_size(pos2(xx + wd / 2., cy), vec2(*wd, *wd));
            xx += wd + sp;
        }
        if self.shuf { accent(p, rs[0], 21., c, 1.) } else { glass(p, rs[0], 21., c, 1.) }
        icon(p, "shuf", rs[0].center(), 22., if self.shuf { c.acon } else { c.tx });
        glass(p, rs[1], 26., c, 1.);
        icon(p, "prev", rs[1].center(), 24., c.tx);
        if now < self.fx_until {
            let t = (((now - (self.fx_until - 5.)) % 2.2) / 2.2) as f32;
            let s = 24. * t;
            p.circle_stroke(rs[2].center(), 37. + s / 2., Stroke::new(s.max(0.1), fade(c.glow, 1. - t)));
        }
        accent(p, rs[2], 37., c, 1.);
        icon(p, if self.playing { "pause" } else { "play" }, rs[2].center(), 30., c.acon);
        glass(p, rs[3], 26., c, 1.);
        icon(p, "next", rs[3].center(), 24., c.tx);
        if self.rep > 0 { accent(p, rs[4], 21., c, 1.) } else { glass(p, rs[4], 21., c, 1.) }
        icon(p, "rep", rs[4].center(), 22., if self.rep > 0 { c.acon } else { c.tx });
        if self.rep == 2 {
            p.text(pos2(rs[4].right() - 10., rs[4].top() + 10.), Align2::CENTER_CENTER, "1", f(11.), c.acon);
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
        y += 74. + gap;

        // volume
        let vx0 = cx - w * 0.39;
        let vx1 = cx + w * 0.39;
        icon(p, "vol", pos2(vx0 + 10., y + 18.), 20., c.mu);
        let vl = Rect::from_min_max(pos2(vx0 + 38., y), pos2(vx1, y + 36.));
        let vline = Rect::from_center_size(pos2(vl.center().x, y + 18.), vec2(vl.width(), 6.));
        slider(p, vline, self.vol, c);
        let vr = ui.interact(vl, Id::new("vol"), Sense::click_and_drag());
        if let Some(pp) = vr.interact_pointer_pos() {
            if vr.dragged() || vr.clicked() {
                self.vol = ((pp.x - vline.left()) / vline.width()).clamp(0., 1.);
                if let Some(s) = self.audio.as_ref().and_then(|a| a.sink.as_ref()) {
                    s.set_volume(self.vol);
                }
            }
        }
    }

    fn splash(&self, p: &Painter, sr: Rect, c: &C, now: f64, st: f32) {
        let a = if st < 2.2 { 1. } else { (1. - (st - 2.2) / 0.5).clamp(0., 1.) };
        bg(p, sr, c, now, a);
        glow(p, pos2(sr.right(), sr.top()), sr.width() * 0.9, c.a1, 0.75 * a);
        glow(p, pos2(sr.left(), sr.bottom()), sr.width() * 0.9, c.a2, 0.6 * a);
        let cx = sr.center().x;
        let cy = sr.center().y - 40.;
        let logo = Rect::from_center_size(pos2(cx, cy), vec2(120., 120.));
        glass(p, logo, 60., c, a);
        disc(p, logo.center(), 49., now as f32 * TAU / 10., c, a);
        let f = |s: f32| FontId::proportional(s);
        p.text(pos2(cx, cy + 92.), Align2::CENTER_CENTER, APP_NAME, f(30.), fade(c.tx, a));
        p.text(pos2(cx, cy + 128.), Align2::CENTER_CENTER, "Written entirely in Rust", f(16.), fade(c.tx, a));
        p.text(pos2(cx, cy + 152.), Align2::CENTER_CENTER, "Fast, light and powerful", f(16.), fade(c.tx, a));
        let bar = Rect::from_center_size(pos2(cx, cy + 186.), vec2(200., 6.));
        p.rect_filled(bar, 3., fade(rgba(0x808080, 0.28), a));
        let fr = Rect::from_min_size(bar.min, vec2(200. * ease((st / 1.9).min(1.)), 6.));
        fill(p, fr, 3., |q| fade(mix(c.a1, c.a2, (q.x - bar.left()) / 200.), a));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        let t0 = *self.t0.get_or_insert(now);
        let dt = (now - self.now).clamp(0., 0.1) as f32;
        self.now = now;
        while let Ok(v) = self.rx.try_recv() {
            self.songs = v;
        }
        if self.playing && self.audio.as_ref().map_or(false, |a| a.done()) {
            if self.rep == 2 { self.play_index(self.cur) } else { self.next(true) }
        }
        if self.playing {
            self.rot += dt * TAU / 10.;
        }
        let c = theme(self.dark, self.pal);
        let st = (now - t0) as f32;
        CentralPanel::default().frame(Frame::none()).show(ctx, |ui| self.draw(ui, &c, now, st));
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

// ---------------------------------------------------------------- Android entry point

/// Asks Android for permission to read the user's audio files (needed to list the music).
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
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(App::new(cc, handle))))).unwrap();
}
