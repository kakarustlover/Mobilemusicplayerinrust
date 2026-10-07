//! Velora - music player. Dioxus (Rust) app: the glass UI lives in `ui/app.html`,
//! Rust does everything native: permission, library scan, tags/covers, audio playback, settings.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use dioxus::prelude::*;
use lofty::prelude::*;
use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

static UI: &str = include_str!("../ui/app.html");
static FONT_R: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static FONT_S: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
static FONT_E: &[u8] = include_bytes!("../assets/fonts/Inter-ExtraBold.ttf");

static TX: OnceLock<UnboundedSender<Value>> = OnceLock::new();
static RX: Mutex<Option<UnboundedReceiver<Value>>> = Mutex::new(None);
static CMD: OnceLock<mpsc::Sender<Cmd>> = OnceLock::new();
static SONGS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static STARTED: AtomicBool = AtomicBool::new(false);

enum Cmd {
    Play(usize, PathBuf),
    Pause,
    Resume,
    Seek(i32),
}

/// Send an event to the UI.
fn emit(v: Value) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(v);
    }
}

fn main() {
    let (tx, rx) = unbounded_channel();
    let _ = TX.set(tx);
    *RX.lock().unwrap() = Some(rx);
    let (ctx, crx) = mpsc::channel();
    let _ = CMD.set(ctx);
    std::thread::spawn(move || player_thread(crx));
    dioxus::launch(app);
}

fn app() -> Element {
    use_hook(|| {
        spawn(async move {
            bridge().await;
        });
    });
    rsx! { div { id: "velora-root" } }
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> &'a str {
    let i = s.find(a).map(|i| i + a.len()).unwrap_or(0);
    let j = s[i..].find(b).map(|j| i + j).unwrap_or(s.len());
    &s[i..j]
}

fn font_css() -> String {
    let mut out = String::new();
    for (w, d) in [(400, FONT_R), (600, FONT_S), (800, FONT_E)] {
        out.push_str(&format!("@font-face{{font-family:'Inter';font-weight:{w};font-style:normal;src:url(data:font/ttf;base64,{}) format('truetype')}}\n", B64.encode(d)));
    }
    out
}

async fn bridge() {
    let css = format!("{}\n{}", font_css(), between(UI, "<style>", "</style>"));
    let body = between(UI, "<body>", "<script>");
    let js = between(UI, "<script>", "</script>");
    let boot = format!(
        "window.__send=function(m){{dioxus.send(m)}};\n\
         (function(){{var s=document.createElement('style');s.textContent={};document.head.appendChild(s);\n\
         document.body.insertAdjacentHTML('beforeend',{});}})();\n{}",
        serde_json::to_string(&css).unwrap(),
        serde_json::to_string(body).unwrap(),
        js
    );
    let mut ev = dioxus::document::eval(&boot);

    // Rust -> UI
    spawn(async move {
        let rx = RX.lock().unwrap().take();
        if let Some(mut rx) = rx {
            while let Some(v) = rx.recv().await {
                let s = serde_json::to_string(&v).unwrap_or_default();
                let _ = dioxus::document::eval(&format!("window.__vEv&&window.__vEv({s})"));
            }
        }
    });

    // UI -> Rust
    while let Ok(m) = ev.recv::<Value>().await {
        handle(m);
    }
}

fn handle(m: Value) {
    let cmd = |c: Cmd| {
        if let Some(t) = CMD.get() {
            let _ = t.send(c);
        }
    };
    match m["t"].as_str().unwrap_or("") {
        "ready" => on_ready(),
        "play" => {
            let i = m["i"].as_u64().unwrap_or(0) as usize;
            let p = SONGS.lock().unwrap().get(i).cloned();
            if let Some(p) = p {
                cmd(Cmd::Play(i, p));
            }
        }
        "pause" => cmd(Cmd::Pause),
        "resume" => cmd(Cmd::Resume),
        "seek" => cmd(Cmd::Seek((m["s"].as_f64().unwrap_or(0.) * 1000.) as i32)),
        "pal" | "mode" => {
            let key = m["t"].as_str().unwrap_or("").to_string();
            let mut st = load_settings();
            st[&key] = m["v"].clone();
            save_settings(&st);
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ settings

fn settings_path() -> Option<PathBuf> {
    let dir = with_env(|env, ctx| {
        let f = env.call_method(ctx, "getFilesDir", "()Ljava/io/File;", &[])?.l()?;
        let p = env.call_method(&f, "getAbsolutePath", "()Ljava/lang/String;", &[])?.l()?;
        let js = JString::from(p);
        let s: String = env.get_string(&js)?.into();
        Ok(s)
    })?;
    Some(PathBuf::from(dir).join("velora.json"))
}

fn load_settings() -> Value {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn save_settings(v: &Value) {
    if let Some(p) = settings_path() {
        let _ = std::fs::write(p, v.to_string());
    }
}

// ------------------------------------------------------------------ startup / library

fn on_ready() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let st = load_settings();
    emit(json!({"t":"init","pal":st["pal"],"mode":st["mode"]}));
    if let Some((t, b)) = insets() {
        emit(json!({"t":"insets","top":t,"bottom":b}));
    }
    std::thread::spawn(|| {
        ask_permission();
        let mut ok = has_permission();
        for _ in 0..240 {
            if ok {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            ok = has_permission();
        }
        if !ok {
            emit(json!({"t":"perm","ok":false}));
            return;
        }
        scan();
    });
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
            if ["mp3", "m4a", "aac", "flac", "ogg", "wav", "opus"].contains(&ext.to_lowercase().as_str()) {
                out.push(p);
            }
        }
    }
}

struct Meta {
    path: PathBuf,
    title: String,
    artist: String,
    album: String,
    dur: f32,
}

fn read_meta(p: &Path) -> Meta {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let mut m = Meta { path: p.to_path_buf(), title: stem, artist: "Unknown".into(), album: "Unknown".into(), dur: 0. };
    if let Ok(tf) = lofty::read_from_path(p) {
        m.dur = tf.properties().duration().as_secs_f32();
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            if let Some(x) = tag.title() {
                if !x.trim().is_empty() {
                    m.title = x.to_string();
                }
            }
            if let Some(x) = tag.artist() {
                if !x.trim().is_empty() {
                    m.artist = x.to_string();
                }
            }
            if let Some(x) = tag.album() {
                if !x.trim().is_empty() {
                    m.album = x.to_string();
                }
            }
        }
    }
    if m.dur < 1. {
        m.dur = probe_duration(p);
    }
    m
}

fn cover_uri(p: &Path) -> Option<String> {
    let tf = lofty::read_from_path(p).ok()?;
    let tag = tf.primary_tag().or_else(|| tf.first_tag())?;
    let pic = tag.pictures().first()?;
    let img = image::load_from_memory(pic.data()).ok()?;
    let img = img.resize_to_fill(256, 256, image::imageops::FilterType::Triangle);
    let rgb = image::DynamicImage::ImageRgb8(img.to_rgb8());
    let mut buf = std::io::Cursor::new(Vec::new());
    rgb.write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
    Some(format!("data:image/jpeg;base64,{}", B64.encode(buf.into_inner())))
}

fn scan() {
    let mut files = Vec::new();
    walk(Path::new("/storage/emulated/0"), 0, &mut files);
    let mut metas: Vec<Meta> = files.iter().map(|p| read_meta(p)).collect();
    metas.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    *SONGS.lock().unwrap() = metas.iter().map(|m| m.path.clone()).collect();
    let items: Vec<Value> = metas.iter().map(|m| json!([m.title, m.artist, m.album, m.dur])).collect();
    emit(json!({"t":"songs","items":items}));
    // covers arrive a few at a time so the list is usable immediately
    let mut batch: Vec<Value> = Vec::new();
    for (i, m) in metas.iter().enumerate() {
        if let Some(u) = cover_uri(&m.path) {
            batch.push(json!([i, u]));
        }
        if batch.len() >= 8 {
            emit(json!({"t":"covers","items":std::mem::take(&mut batch)}));
        }
    }
    if !batch.is_empty() {
        emit(json!({"t":"covers","items":batch}));
    }
}

// ------------------------------------------------------------------ Android (JNI)

fn with_env<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Option<R> {
    let cx = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(cx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let ctx = unsafe { JObject::from_raw(cx.context().cast()) };
    let r = f(&mut env, &ctx);
    if r.is_err() {
        let _ = env.exception_clear();
    }
    r.ok()
}

fn perm_name(env: &mut JNIEnv) -> jni::errors::Result<&'static str> {
    let sdk = env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()?;
    Ok(if sdk >= 33 { "android.permission.READ_MEDIA_AUDIO" } else { "android.permission.READ_EXTERNAL_STORAGE" })
}

fn ask_permission() {
    let _ = with_env(|env, ctx| {
        let perm = perm_name(env)?;
        let s = env.new_string(perm)?;
        let arr = env.new_object_array(1, "java/lang/String", &s)?;
        let arr_o = JObject::from(arr);
        env.call_method(ctx, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&arr_o), JValue::Int(1)])?;
        Ok(())
    });
}

fn has_permission() -> bool {
    with_env(|env, ctx| {
        let perm = perm_name(env)?;
        let s = env.new_string(perm)?;
        let r = env.call_method(ctx, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&s)])?.i()?;
        Ok(r == 0)
    })
    .unwrap_or(false)
}

fn probe_duration(p: &Path) -> f32 {
    with_env(|env, _| {
        let r = env.new_object("android/media/MediaMetadataRetriever", "()V", &[])?;
        let res = (|| -> jni::errors::Result<f32> {
            let s = env.new_string(p.to_string_lossy())?;
            env.call_method(&r, "setDataSource", "(Ljava/lang/String;)V", &[JValue::Object(&s)])?;
            let v = env.call_method(&r, "extractMetadata", "(I)Ljava/lang/String;", &[JValue::Int(9)])?.l()?;
            if v.is_null() {
                return Ok(0.);
            }
            let js = JString::from(v);
            let t: String = env.get_string(&js)?.into();
            Ok(t.parse::<f32>().unwrap_or(0.) / 1000.)
        })();
        let _ = env.call_method(&r, "release", "()V", &[]);
        let _ = env.exception_clear();
        res
    })
    .unwrap_or(0.)
}

/// How much of the status / navigation bar covers our page, in CSS pixels.
fn insets() -> Option<(f32, f32)> {
    with_env(|env, ctx| {
        let win = env.call_method(ctx, "getWindow", "()Landroid/view/Window;", &[])?.l()?;
        let dec = env.call_method(&win, "getDecorView", "()Landroid/view/View;", &[])?.l()?;
        let ins = env.call_method(&dec, "getRootWindowInsets", "()Landroid/view/WindowInsets;", &[])?.l()?;
        if ins.is_null() {
            return Err(jni::errors::Error::NullPtr("insets"));
        }
        let mut top = env.call_method(&ins, "getStableInsetTop", "()I", &[])?.i()?;
        let bot = env.call_method(&ins, "getStableInsetBottom", "()I", &[])?.i()?;
        if let Ok(cut) = env.call_method(&ins, "getDisplayCutout", "()Landroid/view/DisplayCutout;", &[]).and_then(|v| v.l()) {
            if !cut.is_null() {
                if let Ok(v) = env.call_method(&cut, "getSafeInsetTop", "()I", &[]).and_then(|v| v.i()) {
                    top = top.max(v);
                }
            }
        }
        let _ = env.exception_clear();
        // where the page really starts / ends on screen
        let content = env.call_method(ctx, "findViewById", "(I)Landroid/view/View;", &[JValue::Int(16908290)])?.l()?;
        let mut loc = [0i32; 2];
        let mut dloc = [0i32; 2];
        for (view, out) in [(&content, &mut loc), (&dec, &mut dloc)] {
            let arr = env.new_int_array(2)?;
            let arr_o = unsafe { JObject::from_raw(arr.as_raw()) };
            env.call_method(view, "getLocationOnScreen", "([I)V", &[JValue::Object(&arr_o)])?;
            env.get_int_array_region(&arr, 0, &mut out[..])?;
        }
        let ch = env.call_method(&content, "getHeight", "()I", &[])?.i()?;
        let dh = env.call_method(&dec, "getHeight", "()I", &[])?.i()?;
        let top_gap = loc[1] - dloc[1];
        let bot_gap = (dloc[1] + dh) - (loc[1] + ch);
        let need_top = (top - top_gap).max(0) as f32;
        let need_bot = (bot - bot_gap).max(0) as f32;
        let res = env.call_method(ctx, "getResources", "()Landroid/content/res/Resources;", &[])?.l()?;
        let dm = env.call_method(&res, "getDisplayMetrics", "()Landroid/util/DisplayMetrics;", &[])?.l()?;
        let density = env.get_field(&dm, "density", "F")?.f()?.max(1.);
        Ok((need_top / density, need_bot / density))
    })
}

// ------------------------------------------------------------------ audio (Android MediaPlayer)

fn mp_load(path: &Path) -> Option<(GlobalRef, i32)> {
    with_env(|env, ctx| {
        let mp = env.new_object("android/media/MediaPlayer", "()V", &[])?;
        let p = env.new_string(path.to_string_lossy())?;
        if let Err(e) = env.call_method(&mp, "setDataSource", "(Ljava/lang/String;)V", &[JValue::Object(&p)]).and_then(|_| env.call_method(&mp, "prepare", "()V", &[])) {
            let _ = env.exception_clear();
            let _ = env.call_method(&mp, "release", "()V", &[]);
            let _ = env.exception_clear();
            return Err(e);
        }
        let _ = env.call_method(&mp, "setWakeMode", "(Landroid/content/Context;I)V", &[JValue::Object(ctx), JValue::Int(1)]);
        let _ = env.exception_clear();
        env.call_method(&mp, "start", "()V", &[])?;
        let dur = env.call_method(&mp, "getDuration", "()I", &[])?.i()?;
        let g = env.new_global_ref(&mp)?;
        Ok((g, dur))
    })
}

fn mp_call(g: &GlobalRef, name: &str) {
    let _ = with_env(|env, _| {
        env.call_method(g.as_obj(), name, "()V", &[])?;
        Ok(())
    });
}

fn mp_state(g: &GlobalRef) -> Option<(i32, bool)> {
    with_env(|env, _| {
        let pos = env.call_method(g.as_obj(), "getCurrentPosition", "()I", &[])?.i()?;
        let pl = env.call_method(g.as_obj(), "isPlaying", "()Z", &[])?.z()?;
        Ok((pos, pl))
    })
}

fn player_thread(rx: mpsc::Receiver<Cmd>) {
    let mut mp: Option<GlobalRef> = None;
    let (mut want, mut idx, mut dur) = (false, 0usize, 0i32);
    let mut tick = 0u32;
    let mut last_ins: Option<(i32, i32)> = None;
    loop {
        let mut touched = false;
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Cmd::Play(i, path)) => {
                if let Some(old) = mp.take() {
                    mp_call(&old, "release");
                }
                touched = true;
                match mp_load(&path) {
                    Some((g, d)) => {
                        mp = Some(g);
                        dur = d.max(0);
                        idx = i;
                        want = true;
                    }
                    None => {
                        want = false;
                        emit(json!({"t":"err"}));
                    }
                }
            }
            Ok(Cmd::Pause) => {
                want = false;
                touched = true;
                if let Some(m) = &mp {
                    mp_call(m, "pause");
                }
            }
            Ok(Cmd::Resume) => {
                want = true;
                touched = true;
                if let Some(m) = &mp {
                    mp_call(m, "start");
                }
            }
            Ok(Cmd::Seek(ms)) => {
                touched = true;
                if let Some(m) = &mp {
                    let _ = with_env(|env, _| {
                        env.call_method(m.as_obj(), "seekTo", "(I)V", &[JValue::Int(ms)])?;
                        Ok(())
                    });
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some(m) = &mp {
            if want || touched {
                if let Some((pos, playing)) = mp_state(m) {
                    let ended = want && !playing && dur > 0 && (pos + 1500 >= dur || pos == 0);
                    if ended {
                        want = false;
                    }
                    emit(json!({"t":"pos","i":idx,"pos":pos as f64 / 1000.,"dur":dur as f64 / 1000.,"ended":ended}));
                }
            }
        }
        tick += 1;
        if tick % 8 == 0 && STARTED.load(Ordering::SeqCst) {
            if let Some((t, b)) = insets() {
                let key = ((t * 10.) as i32, (b * 10.) as i32);
                if last_ins != Some(key) {
                    last_ins = Some(key);
                    emit(json!({"t":"insets","top":t,"bottom":b}));
                }
            }
        }
    }
}
