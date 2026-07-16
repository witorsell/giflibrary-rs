use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{delete, get, post, put},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use axum_extra::extract::Multipart;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{Client as S3Client, config::Credentials};
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    process::Command,
    sync::Arc,
};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    s3: S3Client,
    bucket: String,
    master_key: String,
    r2_public_url: String,
    db_mutex: Arc<Mutex<()>>,
    rate_limits: Arc<std::sync::Mutex<HashMap<String, Vec<std::time::Instant>>>>,
    global_uploads: Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
    cached_gifs: Arc<Mutex<Option<Vec<(String, i64, i64)>>>>,
}

#[derive(Serialize, Clone)]
struct GifItem {
    key: String,
    url: String,
    #[serde(rename = "lastModified")]
    last_modified: String,
    size: i64,
    tags: Vec<String>,
    slug: String,
    #[serde(rename = "shortKey")]
    short_key: String,
    #[serde(rename = "isNsfwPlaceholder", skip_serializing_if = "Option::is_none")]
    is_nsfw_placeholder: Option<bool>,
    #[serde(rename = "isHidden", skip_serializing_if = "Option::is_none")]
    is_hidden: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    h: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Suggestion {
    tags: Vec<String>,
    #[serde(rename = "sentBy")]
    sent_by: String,
    date: i64,
}

#[derive(Deserialize, Serialize)]
struct Dim {
    w: f64,
    h: f64,
}

fn get_dims_db() -> HashMap<String, Dim> {
    let path = "dimensions.json";
    if let Ok(data) = fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_dims_db(db: &HashMap<String, Dim>) {
    if let Ok(json) = serde_json::to_string_pretty(db) {
        let _ = fs::write("dimensions.json", json);
    }
}

fn hash_string(s: &str) -> i32 {
    let mut hash: i32 = 0;
    for c in s.chars() {
        hash = (c as i32).wrapping_add((hash << 5).wrapping_sub(hash));
    }
    hash
}

fn get_db() -> HashMap<String, Vec<String>> {
    let path = "data.json";
    if let Ok(data) = fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_db(db: &HashMap<String, Vec<String>>) {
    if let Ok(json) = serde_json::to_string_pretty(db) {
        let _ = fs::write("data.json", json);
    }
}

fn get_suggestions_db() -> HashMap<String, Suggestion> {
    let path = "suggestions.json";
    if let Ok(data) = fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_suggestions_db(db: &HashMap<String, Suggestion>) {
    if let Ok(json) = serde_json::to_string_pretty(db) {
        let _ = fs::write("suggestions.json", json);
    }
}

fn convert_to_webp(tmp_in: &str, tmp_out: &str, content_type: &str, is_animated: bool) -> Option<Vec<u8>> {
    let mut args = vec!["-y"];
    
    if is_animated && (content_type == "image/gif" || content_type == "image/webp") {
        args.extend_from_slice(&["-ignore_loop", "0"]);
    }
    
    let comp_level = "6";
    let quality = "90";
    
    if content_type == "image/webp" && is_animated {
        let _ = fs::copy(tmp_in, tmp_out);
        return fs::read(tmp_out).ok();
    } else if content_type == "image/webp" || content_type == "image/jpeg" || content_type == "image/png" {
        args.extend_from_slice(&["-loop", "1", "-r", "2", "-i", tmp_in, "-c:v", "libwebp", "-lossless", "0", "-compression_level", comp_level, "-q:v", quality, "-vframes", "2", "-loop", "0", tmp_out]);
    } else if content_type.starts_with("video/") {
        args.extend_from_slice(&["-i", tmp_in, "-c:v", "libwebp", "-lossless", "0", "-compression_level", comp_level, "-q:v", quality, "-loop", "0", "-an", tmp_out]);
    } else {
        args.extend_from_slice(&["-i", tmp_in, "-c:v", "libwebp", "-lossless", "0", "-compression_level", comp_level, "-q:v", quality, "-loop", "0", tmp_out]);
    }
    
    let status = std::process::Command::new("ffmpeg").args(&args).status();
    
    if status.is_ok() && fs::metadata(tmp_out).is_ok() {
        fs::read(tmp_out).ok()
    } else {
        None
    }
}

async fn auth_status(jar: CookieJar, State(state): State<AppState>) -> impl IntoResponse {
    let logged_in = jar.get("auth_token").map(|c| c.value() == state.master_key).unwrap_or(false);
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(axum::http::header::CACHE_CONTROL, "no-store, no-cache, must-revalidate, private".parse().unwrap());
    (headers, Json(serde_json::json!({ "loggedIn": logged_in })))
}

#[derive(Deserialize)]
struct LoginReq {
    key: String,
}

async fn login(jar: CookieJar, State(state): State<AppState>, Json(payload): Json<LoginReq>) -> (CookieJar, impl IntoResponse) {
    if payload.key == state.master_key {
        let cookie = Cookie::build(("auth_token", payload.key))
            .http_only(true)
            .path("/")
            .build();
        (jar.add(cookie), Json(serde_json::json!({ "success": true })).into_response())
    } else {
        (jar, (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Invalid key" }))).into_response())
    }
}

async fn logout(jar: CookieJar) -> (CookieJar, impl IntoResponse) {
    let cookie = Cookie::build(("auth_token", "")).path("/").build();
    (jar.remove(cookie), Json(serde_json::json!({ "success": true })))
}

fn levenshtein(a: &str, b: &str) -> usize {
    if a.is_empty() { return b.len(); }
    if b.is_empty() { return a.len(); }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut matrix = vec![vec![0; a_chars.len() + 1]; b_chars.len() + 1];
    for i in 0..=b_chars.len() { matrix[i][0] = i; }
    for j in 0..=a_chars.len() { matrix[0][j] = j; }
    for i in 1..=b_chars.len() {
        for j in 1..=a_chars.len() {
            if b_chars[i - 1] == a_chars[j - 1] {
                matrix[i][j] = matrix[i - 1][j - 1];
            } else {
                matrix[i][j] = (matrix[i - 1][j - 1] + 1).min(matrix[i][j - 1] + 1).min(matrix[i - 1][j] + 1);
            }
        }
    }
    matrix[b_chars.len()][a_chars.len()]
}

const NSFW_CATEGORIES: [&str; 4] = ["suggestive", "offensive", "sexual", "nsfw"];

fn gif_nsfw_categories(tags: &[String]) -> Vec<String> {
    tags.iter()
        .map(|t| t.to_lowercase())
        .filter(|t| NSFW_CATEGORIES.contains(&t.as_str()))
        .collect()
}

fn parse_enabled_categories(param: Option<&str>) -> Vec<String> {
    match param {
        Some(s) if !s.trim().is_empty() => s
            .split(',')
            .map(|c| c.trim().to_lowercase())
            .filter(|c| NSFW_CATEGORIES.contains(&c.as_str()))
            .collect(),
        _ => Vec::new(),
    }
}

fn effective_nsfw_categories(gif_categories: &[String]) -> Vec<&String> {
    let specific: Vec<&String> = gif_categories.iter().filter(|c| c.as_str() != "nsfw").collect();
    if specific.is_empty() {
        gif_categories.iter().collect()
    } else {
        specific
    }
}

fn is_locked(gif_categories: &[String], enabled_categories: &[String]) -> bool {
    let effective = effective_nsfw_categories(gif_categories);
    !effective.iter().all(|c| enabled_categories.contains(c))
}

fn nsfw_placeholder_label(gif_categories: &[String]) -> String {
    let effective = effective_nsfw_categories(gif_categories);
    if effective.len() == 1 && effective[0] == "nsfw" {
        "NSFW".to_string()
    } else {
        let parts: Vec<String> = effective.iter().map(|c| c.to_uppercase()).collect();
        format!("NSFW/{}", parts.join("/"))
    }
}

fn nsfw_placeholder_font_size(w: f64, h: f64, label: &str) -> f64 {
    let base = w.min(h) * 0.1;
    let label_len = label.chars().count() as f64;
    // Bold sans-serif glyphs average ~0.62em wide; the 0.15em letter-spacing
    // between characters adds roughly another 0.15em per character.
    let est_width_factor = label_len * 0.77;
    let fit_for_width = (w * 0.92) / est_width_factor;
    base.min(fit_for_width).max(base * 0.6)
}

fn caption_word_tags(caption: &str) -> Vec<String> {
    caption
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

fn merge_caption_tags(existing_tags: &[String], caption: &str) -> Vec<String> {
    let mut tags: Vec<String> = existing_tags.to_vec();
    for word in caption_word_tags(caption) {
        if !tags.contains(&word) {
            tags.push(word);
        }
    }
    if !tags.iter().any(|t| t == "caption") {
        tags.push("caption".to_string());
    }
    tags
}

#[derive(Deserialize)]
struct GifsQuery {
    page: Option<usize>,
    limit: Option<usize>,
    q: Option<String>,
    nsfw_categories: Option<String>,
}

async fn fetch_all_gifs(state: &AppState) -> Result<Vec<(String, i64, i64)>, String> {
    let mut cached = state.cached_gifs.lock().await;
    if let Some(contents) = &*cached {
        return Ok(contents.clone());
    }
    
    let mut all_contents: Vec<(String, i64, i64)> = Vec::new();
    let mut continuation_token: Option<String> = None;
    
    loop {
        let mut req = state.s3.list_objects_v2().bucket(&state.bucket);
        if let Some(token) = continuation_token {
            req = req.continuation_token(token);
        }
        let res = match req.send().await {
            Ok(res) => res,
            Err(_) => return Err("Failed to list objects".to_string()),
        };
        
        for o in res.contents() {
            let k = o.key().unwrap_or_default().to_string();
            let lm = o.last_modified().map(|d| d.as_secs_f64() as i64).unwrap_or(0);
            let s = o.size().unwrap_or(0);
            all_contents.push((k, lm, s));
        }
        
        if res.is_truncated().unwrap_or(false) {
            continuation_token = res.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }
    
    all_contents.sort_by(|a, b| b.1.cmp(&a.1));
    *cached = Some(all_contents.clone());
    Ok(all_contents)
}

async fn get_gifs(jar: CookieJar, Query(q): Query<GifsQuery>, State(state): State<AppState>) -> impl IntoResponse {
    let all_contents = match fetch_all_gifs(&state).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let db = get_db();
    let dims_db = get_dims_db();

    let enabled_categories = parse_enabled_categories(q.nsfw_categories.as_deref());
    let is_admin = jar.get("auth_token").map(|c| c.value() == state.master_key).unwrap_or(false);

    let mut gifs: Vec<GifItem> = all_contents.into_iter().filter_map(|(key, last_mod, size)| {
        let tags = db.get(&key).cloned().unwrap_or_default();

        let gif_categories = gif_nsfw_categories(&tags);
        let is_nsfw = !gif_categories.is_empty();
        let is_hidden = tags.iter().any(|t| t.to_lowercase() == "hidden");

        if is_hidden && !is_admin {
            return None;
        }

        let short_key = key.chars().take(6).collect::<String>();
        let mut slug = short_key.clone();
        
        if !tags.is_empty() {
            let mut safe_tags = Vec::new();
            for t in tags.iter().take(3) {
                let s = t.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "-");
                if !s.is_empty() { safe_tags.push(s); }
            }
            if !safe_tags.is_empty() {
                slug = format!("{}-{}", safe_tags.join("-"), short_key);
            }
        }
        
        let mut url = format!("{}/{}", state.r2_public_url, key);
        let mut is_nsfw_placeholder = None;
        
        if is_nsfw && is_locked(&gif_categories, &enabled_categories) {
            let hash = hash_string(&key).abs();
            let palettes = [
                ["#ff0080", "#7928ca"],
                ["#00dfd8", "#007cf0"],
                ["#ff4d4d", "#f9cb28"],
                ["#ff4b2b", "#ff416c"],
                ["#8e2de2", "#4a00e0"],
                ["#f12711", "#f5af19"],
                ["#12c2e9", "#c471ed"],
                ["#f857a6", "#ff5858"]
            ];
            let palette = palettes[(hash as usize) % palettes.len()];
            let c1 = palette[0];
            let c2 = palette[1];
            
            let w = dims_db.get(&key).map(|d| d.w).unwrap_or(300.0);
            let h = dims_db.get(&key).map(|d| d.h).unwrap_or(400.0);
            
            let label = nsfw_placeholder_label(&gif_categories);
            let tsize = nsfw_placeholder_font_size(w, h, &label);

            let svg_template = r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"> <defs> <filter id="f{hash}" x="-20%" y="-20%" width="140%" height="140%"> <feGaussianBlur stdDeviation="{blur}" /> </filter> </defs> <rect width="{w}" height="{h}" fill="{c2}" /> <circle cx="{cx1}" cy="{cy1}" r="{r1}" fill="{c1}" filter="url(#f{hash})" opacity="0.6" /> <circle cx="{cx2}" cy="{cy2}" r="{r2}" fill="{c2}" filter="url(#f{hash})" opacity="0.6" /> <circle cx="{cx3}" cy="{cy3}" r="{r3}" fill="{c1}" filter="url(#f{hash})" opacity="0.3" /> <rect width="{w}" height="{h}" fill="rgba(0,0,0,0.2)" /> <text x="{tx}" y="{ty}" font-family="sans-serif" font-weight="bold" font-size="{tsize}" fill="#ffffff" opacity="0.3" text-anchor="middle" dominant-baseline="middle" letter-spacing="0.15em">{label}</text> </svg>"##;

            let svg = svg_template
                .replace("{w}", &w.to_string())
                .replace("{h}", &h.to_string())
                .replace("{hash}", &hash.to_string())
                .replace("{c1}", c1)
                .replace("{c2}", c2)
                .replace("{blur}", &w.max(h).mul_add(0.1, 0.0).to_string())
                .replace("{cx1}", &(w*0.8).to_string())
                .replace("{cy1}", &(h*0.2).to_string())
                .replace("{r1}", &(w.min(h)*0.4).to_string())
                .replace("{cx2}", &(w*0.2).to_string())
                .replace("{cy2}", &(h*0.8).to_string())
                .replace("{r2}", &(w.min(h)*0.4).to_string())
                .replace("{cx3}", &(w*0.5).to_string())
                .replace("{cy3}", &(h*0.5).to_string())
                .replace("{r3}", &(w.min(h)*0.3).to_string())
                .replace("{tx}", &(w/2.0).to_string())
                .replace("{ty}", &(h/2.0).to_string())
                .replace("{tsize}", &tsize.to_string())
                .replace("{label}", &label);
            
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let b64 = STANDARD.encode(svg);
            url = format!("data:image/svg+xml;base64,{}", b64);
            is_nsfw_placeholder = Some(true);
        }
        
        let wh = dims_db.get(&key).map(|d| (d.w, d.h));
        Some(GifItem {
            url,
            key,
            last_modified: last_mod.to_string(),
            size,
            tags,
            slug,
            short_key,
            is_nsfw_placeholder,
            is_hidden: if is_hidden { Some(true) } else { None },
            w: wh.map(|(w, _)| w),
            h: wh.map(|(_, h)| h),
        })
    }).collect();

    if let Some(query) = &q.q {
        if !query.trim().is_empty() {
            let terms: Vec<String> = query.split_whitespace().map(|s| s.to_lowercase()).collect();
            gifs.retain(|g| {
                if g.tags.is_empty() { return false; }
                terms.iter().all(|term| {
                    g.tags.iter().any(|t| {
                        let t_lower = t.to_lowercase();
                        if t_lower == *term || t_lower.starts_with(term) { return true; }
                        if term.len() >= 3 && t_lower.contains(term) { return true; }
                        
                        let t_len = t_lower.chars().count();
                        let term_len = term.chars().count();
                        
                        if term.len() >= 4 && (t_len as isize - term_len as isize).abs() <= 1 {
                            return levenshtein(&t_lower, term) <= 1;
                        } else if term.len() >= 7 && (t_len as isize - term_len as isize).abs() <= 2 {
                            return levenshtein(&t_lower, term) <= 2;
                        }
                        
                        false
                    })
                })
            });
        }
    }
    
    let page = q.page.unwrap_or(1).max(1);
    let limit = q.limit.unwrap_or(20);
    let start_index = (page - 1) * limit;
    let end_index = page * limit;
    
    let items = if start_index < gifs.len() {
        gifs[start_index..end_index.min(gifs.len())].to_vec()
    } else {
        Vec::new()
    };
    
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Cache-Control", "no-store, no-cache, must-revalidate, private".parse().unwrap());
    
    (headers, Json(serde_json::json!({
        "gifs": items,
        "total": gifs.len(),
        "totalPages": (gifs.len() as f64 / limit as f64).ceil() as usize,
        "currentPage": page
    }))).into_response()
}

async fn media_proxy(Path(key): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    let url = format!("{}/{}", state.r2_public_url, key);
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("Cache-Control", "public, max-age=31536000".parse().unwrap());
    
    if let Ok(res) = reqwest::get(&url).await {
        if let Ok(bytes) = res.bytes().await {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "image/gif".parse().unwrap());
            headers.insert(header::CACHE_CONTROL, "public, max-age=31536000".parse().unwrap());
            return (headers, bytes.to_vec()).into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn upload_gif(
    jar: CookieJar,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    if jar.get("auth_token").map(|c| c.value()) != Some(&state.master_key) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    
    {
        let mut global_uploads = state.global_uploads.lock().unwrap();
        let now = std::time::Instant::now();
        let one_hour = std::time::Duration::from_secs(3600);
        global_uploads.retain(|t| now.duration_since(*t) < one_hour);
        if global_uploads.len() >= 50 {
            return (StatusCode::TOO_MANY_REQUESTS, "Uploads temporarily disabled due to high volume").into_response();
        }
        global_uploads.push(now);
    }
    
    let mut file_data: Option<axum::body::Bytes> = None;
    let mut content_type = String::new();
    let mut tags: Vec<String> = Vec::new();
    
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "gif" {
            content_type = field.content_type().unwrap_or("").to_string();
            if let Ok(bytes) = field.bytes().await {
                file_data = Some(bytes);
            }
        } else if name == "tags" {
            if let Ok(text) = field.text().await {
                tags = text.split(',').map(|s: &str| s.trim().to_lowercase()).filter(|s: &String| !s.is_empty()).collect();
            }
        }
    }
    
    let Some(data) = file_data else {
        return (StatusCode::BAD_REQUEST, "No file").into_response();
    };
    
    let allowed = ["image/gif", "image/jpeg", "image/png", "image/webp", "video/mp4", "video/webm", "video/quicktime"];
    if !allowed.contains(&content_type.as_str()) {
        return (StatusCode::BAD_REQUEST, "Invalid file type. Only images and videos are allowed.").into_response();
    }

    let tmp_in = format!("/tmp/in_{}.tmp", hex::encode(rand::random::<[u8; 4]>()));
    let tmp_out = format!("/tmp/out_{}.webp", hex::encode(rand::random::<[u8; 4]>()));
    
    fs::write(&tmp_in, &data).unwrap();
    let is_animated = data.windows(4).any(|w| w == b"ANIM");
    
    let final_data = convert_to_webp(&tmp_in, &tmp_out, &content_type, is_animated).unwrap_or_else(|| data.to_vec());
    
    let _ = fs::remove_file(&tmp_in);
    let _ = fs::remove_file(&tmp_out);
    
    let key = format!("{}.webp", hex::encode(rand::random::<[u8; 8]>()));
    
    let _guard = state.db_mutex.lock().await;
    
    if let Ok(_) = state.s3.put_object()
        .bucket(&state.bucket)
        .key(&key)
        .body(final_data.clone().into())
        .content_type("image/webp")
        .send().await 
    {
        let mut db = get_db();
        db.insert(key.clone(), tags.clone());
        save_db(&db);

        if let Ok(img) = image::load_from_memory(&final_data) {
            let mut dims_db = get_dims_db();
            dims_db.insert(key.clone(), Dim { w: img.width() as f64, h: img.height() as f64 });
            save_dims_db(&dims_db);
        }

        *state.cached_gifs.lock().await = None;

        Json(serde_json::json!({
            "success": true,
            "url": format!("{}/{}", state.r2_public_url, key),
            "key": key,
            "tags": tags
        })).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "Upload failed").into_response()
    }
}

async fn suggest_gif(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let ip = headers.get("cf-connecting-ip")
        .and_then(|hv| hv.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            headers.get("x-forwarded-for")
                .and_then(|hv| hv.to_str().ok())
                .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });
        
    {
        let mut limits = state.rate_limits.lock().unwrap();
        let now = std::time::Instant::now();
        let one_hour = std::time::Duration::from_secs(3600);
        
        let entries = limits.entry(ip).or_insert_with(Vec::new);
        entries.retain(|t| now.duration_since(*t) < one_hour);
        
        if entries.len() >= 5 {
            return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "error": "Too many suggestions from this IP, please try again after an hour." }))).into_response();
        }
        entries.push(now);
    }
    
    {
        let mut global_uploads = state.global_uploads.lock().unwrap();
        let now = std::time::Instant::now();
        let one_hour = std::time::Duration::from_secs(3600);
        global_uploads.retain(|t| now.duration_since(*t) < one_hour);
        if global_uploads.len() >= 50 {
            return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "error": "Uploads temporarily disabled due to high volume" }))).into_response();
        }
        global_uploads.push(now);
    }
    
    let _guard = state.db_mutex.lock().await;
    let mut sdb = get_suggestions_db();
    if sdb.len() >= 50 {
        return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({ "error": "Queue full" }))).into_response();
    }
    
    let mut file_data: Option<axum::body::Bytes> = None;
    let mut content_type = String::new();
    let mut tags: Vec<String> = Vec::new();
    let mut sent_by = String::new();
    
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "gif" {
            content_type = field.content_type().unwrap_or("").to_string();
            if let Ok(bytes) = field.bytes().await {
                file_data = Some(bytes);
            }
        } else if name == "tags" {
            if let Ok(text) = field.text().await {
                tags = text.split(',')
                    .map(|s: &str| s.trim().to_lowercase())
                    .filter(|s: &String| !s.is_empty())
                    .map(|s: String| s.chars().take(30).collect())
                    .take(10)
                    .collect();
            }
        } else if name == "sentBy" {
            if let Ok(text) = field.text().await {
                sent_by = text.chars().take(40).collect();
            }
        }
    }
    
    let Some(data) = file_data else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "No file" }))).into_response();
    };
    
    let allowed = ["image/gif", "image/jpeg", "image/png", "image/webp", "video/mp4", "video/webm", "video/quicktime"];
    if !allowed.contains(&content_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "Invalid file type. Only images and videos are allowed." }))).into_response();
    }
    if sent_by.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "Name required" }))).into_response();
    }

    let tmp_in = format!("/tmp/in_{}.tmp", hex::encode(rand::random::<[u8; 4]>()));
    let tmp_out = format!("/tmp/out_{}.webp", hex::encode(rand::random::<[u8; 4]>()));
    
    fs::write(&tmp_in, &data).unwrap();
    let is_animated = data.windows(4).any(|w| w == b"ANIM");
    
    let final_data = convert_to_webp(&tmp_in, &tmp_out, &content_type, is_animated).unwrap_or_else(|| data.to_vec());
    
    let _ = fs::remove_file(&tmp_in);
    let _ = fs::remove_file(&tmp_out);
    
    let key = format!("{}.webp", hex::encode(rand::random::<[u8; 8]>()));
    
    if let Ok(_) = state.s3.put_object()
        .bucket(&state.bucket)
        .key(&key)
        .body(final_data.clone().into())
        .content_type("image/webp")
        .send().await 
    {
        sdb.insert(key.clone(), Suggestion {
            tags,
            sent_by,
            date: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64,
        });
        save_suggestions_db(&sdb);

        if let Ok(img) = image::load_from_memory(&final_data) {
            let mut dims_db = get_dims_db();
            dims_db.insert(key.clone(), Dim { w: img.width() as f64, h: img.height() as f64 });
            save_dims_db(&dims_db);
        }
        
        Json(serde_json::json!({ "success": true, "message": "Suggestion submitted successfully!" })).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": "Upload failed" }))).into_response()
    }
}

async fn get_suggestions(jar: CookieJar, State(state): State<AppState>) -> impl IntoResponse {
    if jar.get("auth_token").map(|c| c.value()) != Some(&state.master_key) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let sdb = get_suggestions_db();
    let mut list = sdb.into_iter().map(|(key, s)| {
        serde_json::json!({
            "key": key,
            "tags": s.tags,
            "sentBy": s.sent_by,
            "date": s.date,
            "url": format!("{}/{}", state.r2_public_url, key)
        })
    }).collect::<Vec<_>>();
    list.sort_by(|a, b| b["date"].as_i64().unwrap_or(0).cmp(&a["date"].as_i64().unwrap_or(0)));
    Json(serde_json::json!({ "success": true, "suggestions": list })).into_response()
}

async fn approve_suggestion(jar: CookieJar, Path(key): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    if jar.get("auth_token").map(|c| c.value()) != Some(&state.master_key) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let _guard = state.db_mutex.lock().await;
    let mut sdb = get_suggestions_db();
    if let Some(s) = sdb.remove(&key) {
        let mut db = get_db();
        db.insert(key.clone(), s.tags);
        save_db(&db);
        save_suggestions_db(&sdb);
        *state.cached_gifs.lock().await = None;
        Json(serde_json::json!({ "success": true })).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Not found").into_response()
    }
}

async fn reject_suggestion(jar: CookieJar, Path(key): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    if jar.get("auth_token").map(|c| c.value()) != Some(&state.master_key) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let _guard = state.db_mutex.lock().await;
    let mut sdb = get_suggestions_db();
    if sdb.remove(&key).is_some() {
        let _ = state.s3.delete_object().bucket(&state.bucket).key(&key).send().await;
        save_suggestions_db(&sdb);
        Json(serde_json::json!({ "success": true })).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Not found").into_response()
    }
}

#[derive(Deserialize)]
struct TagsUpdate {
    tags: String,
}

async fn update_tags(jar: CookieJar, Path(key): Path<String>, State(state): State<AppState>, Json(payload): Json<TagsUpdate>) -> impl IntoResponse {
    if jar.get("auth_token").map(|c| c.value()) != Some(&state.master_key) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let _guard = state.db_mutex.lock().await;
    let mut db = get_db();
    let parsed_tags = payload.tags.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>();
    db.insert(key, parsed_tags);
    save_db(&db);
    *state.cached_gifs.lock().await = None;
    Json(serde_json::json!({ "success": true })).into_response()
}

async fn delete_gif(jar: CookieJar, Path(key): Path<String>, State(state): State<AppState>) -> impl IntoResponse {
    if jar.get("auth_token").map(|c| c.value()) != Some(&state.master_key) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let _guard = state.db_mutex.lock().await;
    let mut db = get_db();
    db.remove(&key);
    save_db(&db);
    let _ = state.s3.delete_object().bucket(&state.bucket).key(&key).send().await;
    *state.cached_gifs.lock().await = None;
    Json(serde_json::json!({ "success": true })).into_response()
}

async fn serve_html(jar: CookieJar, Path(slug_param): Path<String>, headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    let slug_lower = slug_param.to_lowercase();
    let has_ext = slug_lower.ends_with(".webp") || slug_lower.ends_with(".gif") || slug_lower.ends_with(".mp4") || slug_lower.ends_with(".webm");
    
    let target_slug = if has_ext {
        let mut s = slug_lower.clone();
        if s.ends_with(".webp") { s.truncate(s.len() - 5); }
        else if s.ends_with(".gif") || s.ends_with(".mp4") { s.truncate(s.len() - 4); }
        else if s.ends_with(".webm") { s.truncate(s.len() - 5); }
        s
    } else {
        slug_lower.clone()
    };
    
    let all_contents = match fetch_all_gifs(&state).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    
    let db = get_db();
    
    let mut matched_gif = None;
    for (key, _, _) in all_contents {
        let tags = db.get(&key).cloned().unwrap_or_default();
        let short_key = key.chars().take(6).collect::<String>();
        let mut slug = short_key.clone();
        
        if !tags.is_empty() {
            let mut safe_tags = Vec::new();
            for t in tags.iter().take(3) {
                let s = t.to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "-");
                if !s.is_empty() { safe_tags.push(s); }
            }
            if !safe_tags.is_empty() {
                slug = format!("{}-{}", safe_tags.join("-"), short_key);
            }
        }
        
        if slug == target_slug || short_key == target_slug || key.to_lowercase().starts_with(&target_slug) {
            matched_gif = Some((key, slug, tags));
            break;
        }
    }
    
    let (key, slug, tags) = match matched_gif {
        Some(g) => g,
        None => return not_found_page().into_response(),
    };

    let is_hidden = tags.iter().any(|t| t.to_lowercase() == "hidden");
    let is_admin = jar.get("auth_token").map(|c| c.value() == state.master_key).unwrap_or(false);
    if is_hidden && !is_admin {
        return not_found_page().into_response();
    }

    let raw_url = format!("{}/{}", state.r2_public_url, key);
    
    if has_ext {
        let user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
        let accept = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
        let is_discord = user_agent.contains("discord");
        
        if accept.contains("html") && !is_discord {
            return axum::response::Redirect::permanent(&format!("/gif/{}", slug)).into_response();
        }
        
        return axum::response::Redirect::temporary(&raw_url).into_response();
    }
    
    if target_slug != slug {
        return axum::response::Redirect::permanent(&format!("/gif/{}", slug)).into_response();
    }
    
    let is_nsfw = !gif_nsfw_categories(&tags).is_empty();
    let filter_style = if is_nsfw { "filter: brightness(0.85);" } else { "" };
    
    let escaped_tags: Vec<String> = tags.iter().map(|t| t.replace("&", "&amp;")
                                                         .replace("<", "&lt;")
                                                         .replace(">", "&gt;")
                                                         .replace("\"", "&quot;")
                                                         .replace("'", "&#39;")).collect();
    
    let tags_text = if escaped_tags.is_empty() { "GIF".to_string() } else { escaped_tags.join(", ") };
    let tags_html = if escaped_tags.is_empty() {
        "<span class=\"tag\">untagged</span>".to_string()
    } else {
        escaped_tags.iter().map(|t| format!("<span class=\"tag\">{}</span>", t.to_lowercase())).collect::<Vec<_>>().join("")
    };
    
    if let Ok(mut html) = fs::read_to_string("public/gif.html") {
        html = html.replace("{tags_text}", &tags_text);
        html = html.replace("{slug}", &slug);
        html = html.replace("{raw_url}", &raw_url);
        html = html.replace("{filter_style}", filter_style);
        html = html.replace("{tags_html}", &tags_html);
        return Html(html).into_response();
    }

    Html("<h1>500 Template Missing</h1>".to_string()).into_response()
}

#[derive(Deserialize)]
struct TrollPayload {
    payload: String,
}

async fn log_malicious_troll(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<TrollPayload>) -> impl IntoResponse {
    let ip = headers.get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // 10 logs per ip per hour, console spammers get dropped silently
    {
        let mut limits = state.rate_limits.lock().unwrap();
        let now = std::time::Instant::now();
        let one_hour = std::time::Duration::from_secs(3600);
        let entries = limits.entry(format!("troll:{}", ip)).or_insert_with(Vec::new);
        entries.retain(|t| now.duration_since(*t) < one_hour);
        if entries.len() >= 10 {
            return StatusCode::OK;
        }
        entries.push(now);
    }

    let payload: String = body.payload.chars().take(300).collect();

    let entry = serde_json::json!({
        "ip": ip,
        "payload": payload,
        "timestamp": chrono::Utc::now().timestamp()
    });

    let mut logs: Vec<serde_json::Value> = fs::read_to_string("malicious-ppl.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(Vec::new);

    logs.push(entry);
    if logs.len() > 1000 {
        let excess = logs.len() - 1000;
        logs.drain(..excess);
    }
    let _ = fs::write("malicious-ppl.json", serde_json::to_string_pretty(&logs).unwrap_or_default());

    StatusCode::OK
}

fn not_found_page() -> impl IntoResponse {
    let content = fs::read_to_string("public/404.html").unwrap_or_else(|_| "404 Not Found".to_string());
    (StatusCode::NOT_FOUND, Html(content))
}

async fn serve_about() -> impl IntoResponse {
    let email = std::env::var("CONTACT_EMAIL").unwrap_or_else(|_| "admin@example.com".to_string());
    if let Ok(content) = fs::read_to_string("public/about.html") {
        let content = content.replace("{{CONTACT_EMAIL}}", &email);
        Html(content).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Not found").into_response()
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    
    let r2_endpoint = std::env::var("R2_ENDPOINT").expect("R2_ENDPOINT missing");
    let r2_access = std::env::var("R2_ACCESS_KEY_ID").expect("R2_ACCESS_KEY_ID missing");
    let r2_secret = std::env::var("R2_SECRET_ACCESS_KEY").expect("R2_SECRET_ACCESS_KEY missing");
    let r2_bucket = std::env::var("R2_BUCKET").expect("R2_BUCKET missing");
    let r2_public_url = std::env::var("R2_PUBLIC_URL").expect("R2_PUBLIC_URL missing");
    let master_key = std::env::var("MASTER_KEY").expect("MASTER_KEY missing");
    
    let config = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(r2_endpoint)
        .credentials_provider(Credentials::new(r2_access, r2_secret, None, None, "r2"))
        .region(aws_sdk_s3::config::Region::new("auto"))
        .load()
        .await;
        
    let s3 = S3Client::new(&config);
    
    let state = AppState {
        s3,
        bucket: r2_bucket,
        master_key,
        r2_public_url,
        db_mutex: Arc::new(Mutex::new(())),
        rate_limits: Arc::new(std::sync::Mutex::new(HashMap::new())),
        global_uploads: Arc::new(std::sync::Mutex::new(Vec::new())),
        cached_gifs: Arc::new(Mutex::new(None)),
    };
    
    let app = Router::new()
        .route("/api/auth/status", get(auth_status))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/gifs", get(get_gifs))
        .route("/media/{key}", get(media_proxy))
        .route("/gif/{slug}", get(serve_html))
        .route("/api/upload", post(upload_gif))
        .route("/api/suggest", post(suggest_gif))
        .route("/api/suggestions", get(get_suggestions))
        .route("/api/suggestions/{key}/approve", post(approve_suggestion))
        .route("/api/suggestions/{key}/reject", delete(reject_suggestion))
        .route("/api/gifs/{key}/tags", put(update_tags))
        .route("/api/gifs/{key}", delete(delete_gif))
        .route("/api/troll", post(log_malicious_troll))
        .route("/about", get(serve_about))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .fallback_service(ServeDir::new("public").not_found_service(ServeFile::new("public/404.html")))
        .with_state(state)
        .layer(CorsLayer::permissive());
        
    let port_str = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let port: u16 = port_str.parse().unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Server running on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_nsfw_categories_extracts_reserved_tags_case_insensitively() {
        let tags = vec!["meme".to_string(), "NSFW".to_string(), "funny".to_string()];
        assert_eq!(gif_nsfw_categories(&tags), vec!["nsfw".to_string()]);
    }

    #[test]
    fn gif_nsfw_categories_returns_multiple_matches_in_tag_order() {
        let tags = vec!["offensive".to_string(), "sexual".to_string(), "funny".to_string()];
        assert_eq!(
            gif_nsfw_categories(&tags),
            vec!["offensive".to_string(), "sexual".to_string()]
        );
    }

    #[test]
    fn gif_nsfw_categories_empty_for_sfw_gif() {
        let tags = vec!["meme".to_string(), "funny".to_string()];
        assert!(gif_nsfw_categories(&tags).is_empty());
    }

    #[test]
    fn parse_enabled_categories_splits_trims_and_lowercases() {
        assert_eq!(
            parse_enabled_categories(Some("Suggestive, offensive")),
            vec!["suggestive".to_string(), "offensive".to_string()]
        );
    }

    #[test]
    fn parse_enabled_categories_drops_unknown_values() {
        assert_eq!(
            parse_enabled_categories(Some("suggestive,bogus")),
            vec!["suggestive".to_string()]
        );
    }

    #[test]
    fn parse_enabled_categories_empty_for_none_or_blank() {
        assert!(parse_enabled_categories(None).is_empty());
        assert!(parse_enabled_categories(Some("")).is_empty());
    }

    #[test]
    fn is_locked_false_for_sfw_gif() {
        assert!(!is_locked(&[], &[]));
        assert!(!is_locked(&[], &["offensive".to_string()]));
    }

    #[test]
    fn is_locked_true_when_category_not_enabled() {
        let gif_cats = vec!["offensive".to_string()];
        assert!(is_locked(&gif_cats, &[]));
    }

    #[test]
    fn is_locked_false_when_single_category_enabled() {
        let gif_cats = vec!["offensive".to_string()];
        let enabled = vec!["offensive".to_string()];
        assert!(!is_locked(&gif_cats, &enabled));
    }

    #[test]
    fn is_locked_requires_all_of_gifs_categories_enabled() {
        let gif_cats = vec!["offensive".to_string(), "sexual".to_string()];
        let enabled_partial = vec!["offensive".to_string()];
        let enabled_full = vec!["offensive".to_string(), "sexual".to_string()];
        assert!(is_locked(&gif_cats, &enabled_partial));
        assert!(!is_locked(&gif_cats, &enabled_full));
    }

    #[test]
    fn is_locked_nsfw_tag_does_not_add_a_requirement_alongside_a_specific_category() {
        let gif_cats = vec!["sexual".to_string(), "nsfw".to_string()];
        let enabled_sexual_only = vec!["sexual".to_string()];
        assert!(!is_locked(&gif_cats, &enabled_sexual_only));
    }

    #[test]
    fn is_locked_nsfw_only_gif_still_requires_nsfw_enabled() {
        let gif_cats = vec!["nsfw".to_string()];
        assert!(is_locked(&gif_cats, &[]));
        assert!(!is_locked(&gif_cats, &["nsfw".to_string()]));
    }

    #[test]
    fn is_locked_nsfw_does_not_bypass_other_unenabled_specific_categories() {
        let gif_cats = vec!["offensive".to_string(), "sexual".to_string(), "nsfw".to_string()];
        let enabled_offensive_only = vec!["offensive".to_string()];
        let enabled_both = vec!["offensive".to_string(), "sexual".to_string()];
        assert!(is_locked(&gif_cats, &enabled_offensive_only));
        assert!(!is_locked(&gif_cats, &enabled_both));
    }

    #[test]
    fn nsfw_placeholder_label_generic_only() {
        let gif_cats = vec!["nsfw".to_string()];
        assert_eq!(nsfw_placeholder_label(&gif_cats), "NSFW");
    }

    #[test]
    fn nsfw_placeholder_label_single_specific_category() {
        let gif_cats = vec!["sexual".to_string()];
        assert_eq!(nsfw_placeholder_label(&gif_cats), "NSFW/SEXUAL");
    }

    #[test]
    fn nsfw_placeholder_label_drops_generic_when_specific_present() {
        let gif_cats = vec!["sexual".to_string(), "nsfw".to_string()];
        assert_eq!(nsfw_placeholder_label(&gif_cats), "NSFW/SEXUAL");
    }

    #[test]
    fn nsfw_placeholder_label_joins_multiple_specific_categories() {
        let gif_cats = vec!["offensive".to_string(), "sexual".to_string()];
        assert_eq!(nsfw_placeholder_label(&gif_cats), "NSFW/OFFENSIVE/SEXUAL");
    }

    #[test]
    fn nsfw_placeholder_font_size_uses_base_size_when_label_fits() {
        let size = nsfw_placeholder_font_size(850.0, 1403.0, "NSFW");
        assert_eq!(size, 850.0_f64.min(1403.0) * 0.1);
    }

    #[test]
    fn nsfw_placeholder_font_size_shrinks_for_long_labels_but_stays_above_floor() {
        let base = 850.0_f64.min(1403.0) * 0.1;
        let size = nsfw_placeholder_font_size(850.0, 1403.0, "NSFW/OFFENSIVE/SEXUAL");
        assert!(size < base, "expected long label to shrink below base size");
        assert!(size >= base * 0.6, "expected size to respect the readability floor");
    }

    #[test]
    fn nsfw_placeholder_font_size_never_exceeds_base_size() {
        let base = 300.0_f64.min(400.0) * 0.1;
        let size = nsfw_placeholder_font_size(300.0, 400.0, "NSFW/SEXUAL");
        assert!(size <= base);
    }

    #[test]
    fn caption_word_tags_splits_lowercases_and_trims_punctuation() {
        let tags = caption_word_tags("SWIPE UP to get this ringtone!");
        assert_eq!(
            tags,
            vec!["swipe", "up", "to", "get", "this", "ringtone"]
                .into_iter().map(String::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn caption_word_tags_empty_for_blank_input() {
        assert!(caption_word_tags("").is_empty());
        assert!(caption_word_tags("   ").is_empty());
    }

    #[test]
    fn merge_caption_tags_adds_caption_tag() {
        let existing = vec!["meme".to_string()];
        let merged = merge_caption_tags(&existing, "hello world");
        assert!(merged.contains(&"caption".to_string()));
    }

    #[test]
    fn merge_caption_tags_dedupes_against_existing_manual_tags() {
        let existing = vec!["bro".to_string(), "dance".to_string()];
        let merged = merge_caption_tags(&existing, "bro dance party");
        let bro_count = merged.iter().filter(|t| *t == "bro").count();
        assert_eq!(bro_count, 1);
        assert!(merged.contains(&"party".to_string()));
    }

    #[test]
    fn merge_caption_tags_preserves_manual_tags_then_appends_caption_words_then_caption_tag() {
        let existing = vec!["exploit".to_string(), "community".to_string()];
        let merged = merge_caption_tags(&existing, "can ur external do this");
        assert_eq!(
            merged,
            vec!["exploit", "community", "can", "ur", "external", "do", "this", "caption"]
                .into_iter().map(String::from).collect::<Vec<_>>()
        );
    }
}
