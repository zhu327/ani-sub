use std::collections::HashSet;
use std::sync::Arc;

use reqwest::header;
use serde::{Deserialize, Serialize};
use worker::wasm_bindgen::JsValue;
use worker::{event, Env, Request, Response, RouteContext, Router, ScheduleContext, ScheduledEvent};

struct Config {
    prowlarr: Prowlarr,
    ntfy: Ntfy,
}

struct Prowlarr {
    url: String,
    api_key: String,
    indexer: u32,
}

struct Ntfy {
    enable: bool,
    topic: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Anime {
    id: Option<i32>,
    keywords: String,
    exclude_keywords: String,
    indexer: Option<u32>,
}

#[allow(warnings)]
#[warn(non_snake_case)]
#[derive(Debug, Deserialize)]
struct SearchResult {
    age: u32,
    title: String,
    guid: String,
    infoUrl: String,
}

async fn search(
    prowlarr: &Prowlarr,
    indexer: Option<u32>,
    keywords: &str,
) -> std::result::Result<Vec<SearchResult>, reqwest::Error> {
    let url = format!("{}/api/v1/search", prowlarr.url);

    let indexer = indexer.unwrap_or(prowlarr.indexer);

    let params = [("query", keywords), ("indexerIds", &indexer.to_string())];

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .query(&params)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/json")
        .header("X-Api-Key", &prowlarr.api_key)
        .send()
        .await?
        .error_for_status()?;

    let result: Vec<SearchResult> = response.json().await?;
    Ok(result)
}

#[allow(warnings)]
#[warn(non_snake_case)]
#[derive(Debug, Deserialize)]
struct HistoryData {
    infoUrl: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryItem {
    data: HistoryData,
    successful: bool,
}

#[derive(Debug, Deserialize)]
struct HistoryResult {
    records: Vec<HistoryItem>,
}

async fn history(prowlarr: &Prowlarr) -> std::result::Result<Vec<HistoryItem>, reqwest::Error> {
    let url = format!("{}/api/v1/history", prowlarr.url);

    let params = [
        ("eventType", &"1".to_string()),
        ("successful", &"true".to_string()),
        ("page", &"1".to_string()),
        ("pageSize", &"100".to_string()),
    ];

    let client = reqwest::Client::new();
    let result: HistoryResult = client
        .get(&url)
        .query(&params)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/json")
        .header("X-Api-Key", &prowlarr.api_key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(result.records)
}

#[allow(warnings)]
#[warn(non_snake_case)]
#[derive(Debug, Deserialize, Serialize)]
struct DownloadRequest {
    guid: String,
    indexerId: u32,
}

async fn download(
    prowlarr: &Prowlarr,
    indexer: Option<u32>,
    guid: &str,
) -> std::result::Result<(), reqwest::Error> {
    let url = format!("{}/api/v1/search", prowlarr.url);

    let indexer = indexer.unwrap_or(prowlarr.indexer);

    let request_body = DownloadRequest {
        guid: guid.to_string(),
        indexerId: indexer,
    };

    let client = reqwest::Client::new();
    client
        .post(&url)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/json")
        .header("X-Api-Key", &prowlarr.api_key)
        .json(&request_body)
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

async fn send_message(ntfy: &Ntfy, message: &str) -> std::result::Result<(), reqwest::Error> {
    let url = format!("https://ntfy.sh/{}", ntfy.topic);

    let client = reqwest::Client::new();
    client
        .post(&url)
        .body(message.to_string())
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

fn match_exclude_keywords(title: &str, exclude_keywords: &str) -> bool {
    if exclude_keywords.is_empty() {
        return false;
    }

    exclude_keywords
        .split_whitespace()
        .any(|keyword| title.to_lowercase().contains(&keyword.to_lowercase()))
}

async fn process_anime(
    anime: Anime,
    config: &Config,
    history_urls: Arc<HashSet<String>>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let items = search(&config.prowlarr, anime.indexer, &anime.keywords).await?;
    for item in items {
        if item.age > 2 || match_exclude_keywords(&item.title, &anime.exclude_keywords) {
            continue;
        }

        // Check if already downloaded
        if history_urls.contains(&item.infoUrl) {
            continue;
        }

        // Download
        download(&config.prowlarr, anime.indexer, &item.guid).await?;

        // Notify
        if config.ntfy.enable {
            send_message(&config.ntfy, &format!("Downloading {}", item.title)).await?;
        }

        break;
    }

    Ok(())
}

// ============================================================================
// Auth helpers
// ============================================================================

fn parse_cookies(header_value: &str) -> Vec<(&str, &str)> {
    header_value
        .split(';')
        .filter_map(|cookie| {
            let mut parts = cookie.trim().splitn(2, '=');
            let key = parts.next()?.trim();
            let value = parts.next()?.trim();
            Some((key, value))
        })
        .collect()
}

fn get_session_cookie<'a>(cookies: &'a [(&'a str, &'a str)]) -> Option<&'a str> {
    cookies
        .iter()
        .find(|(k, _)| *k == "session")
        .map(|(_, v)| *v)
}

fn is_authenticated(req: &Request, password: &str) -> bool {
    let cookie_header = match req.headers().get("Cookie") {
        Ok(Some(h)) => h,
        _ => return false,
    };
    let cookies = parse_cookies(&cookie_header);
    get_session_cookie(&cookies)
        .map(|s| s == password)
        .unwrap_or(false)
}

// ============================================================================
// Request types
// ============================================================================

#[derive(Debug, Deserialize)]
struct LoginRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct AnimeInput {
    keywords: String,
    exclude_keywords: Option<String>,
    indexer: Option<u32>,
}

// ============================================================================
// Route handlers
// ============================================================================

async fn handle_login(mut req: Request, env: Env) -> worker::Result<Response> {
    let body: LoginRequest = req.json().await?;
    let password = env.var("AUTH_PASSWORD")?.to_string();

    if body.password != password {
        return Response::from_json(&serde_json::json!({"ok": false, "error": "密码错误"}))
            .map(|r| r.with_status(401));
    }

    let cookie = format!(
        "session={}; HttpOnly; Secure; SameSite=Strict; Path=/",
        body.password
    );
    let mut headers = worker::Headers::new();
    headers.set("Set-Cookie", &cookie)?;

    Response::from_json(&serde_json::json!({"ok": true}))
        .map(|r| r.with_status(303).with_headers(headers))
}

async fn handle_logout(_req: Request) -> worker::Result<Response> {
    let mut headers = worker::Headers::new();
    headers.set(
        "Set-Cookie",
        "session=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0",
    )?;

    Response::empty().map(|r| r.with_headers(headers))
}

async fn handle_index(_req: Request, _ctx: RouteContext<()>) -> worker::Result<Response> {
    Response::from_html(MANAGEMENT_HTML)
}

async fn handle_anime_list(_req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let d1 = ctx.d1("DB")?;
    let result = d1.prepare("SELECT * FROM anime").all().await?;
    let animes: Vec<Anime> = result.results::<Anime>()?;
    Response::from_json(&serde_json::json!({"ok": true, "data": animes}))
}

async fn handle_anime_create(mut req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let input: AnimeInput = req.json().await?;
    let d1 = ctx.d1("DB")?;

    let exclude = input.exclude_keywords.unwrap_or_default();
    let indexer_val = match input.indexer {
        Some(i) => JsValue::from_f64(i as f64),
        None => JsValue::null(),
    };
    let params = [
        JsValue::from_str(&input.keywords),
        JsValue::from_str(&exclude),
        indexer_val,
    ];

    d1.prepare("INSERT INTO anime (keywords, exclude_keywords, indexer) VALUES (?, ?, ?)")
        .bind(&params)?
        .run()
        .await?;

    let anime = d1
        .prepare("SELECT * FROM anime WHERE id = last_insert_rowid()")
        .first::<Anime>(None)
        .await?;

    Response::from_json(&serde_json::json!({"ok": true, "data": anime}))
}

async fn handle_anime_update(mut req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let id: i32 = ctx
        .param("id")
        .ok_or_else(|| worker::Error::RustError("missing id".into()))?
        .parse()
        .map_err(|_| worker::Error::RustError("invalid id".into()))?;

    let input: AnimeInput = req.json().await?;
    let d1 = ctx.d1("DB")?;

    let exclude = input.exclude_keywords.unwrap_or_default();
    let indexer_val = match input.indexer {
        Some(i) => JsValue::from_f64(i as f64),
        None => JsValue::null(),
    };
    let params = [
        JsValue::from_str(&input.keywords),
        JsValue::from_str(&exclude),
        indexer_val,
        JsValue::from_f64(id as f64),
    ];

    d1.prepare("UPDATE anime SET keywords = ?, exclude_keywords = ?, indexer = ? WHERE id = ?")
        .bind(&params)?
        .run()
        .await?;

    let anime = d1
        .prepare("SELECT * FROM anime WHERE id = ?")
        .bind(&[JsValue::from_f64(id as f64)])?
        .first::<Anime>(None)
        .await?;

    Response::from_json(&serde_json::json!({"ok": true, "data": anime}))
}

async fn handle_anime_delete(_req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let id: i32 = ctx
        .param("id")
        .ok_or_else(|| worker::Error::RustError("missing id".into()))?
        .parse()
        .map_err(|_| worker::Error::RustError("invalid id".into()))?;

    let d1 = ctx.d1("DB")?;
    d1.prepare("DELETE FROM anime WHERE id = ?")
        .bind(&[JsValue::from_f64(id as f64)])?
        .run()
        .await?;

    Response::from_json(&serde_json::json!({"ok": true}))
}

// ============================================================================
// Fetch handler
// ============================================================================

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: worker::Context) -> worker::Result<Response> {
    let password = env.var("AUTH_PASSWORD")?.to_string();
    let path = req.path();
    let method = req.method();

    // Public routes (no auth required)
    if path == "/login" && method == worker::Method::Post {
        return handle_login(req, env).await;
    }
    if path == "/logout" && method == worker::Method::Post {
        return handle_logout(req).await;
    }

    // Check authentication for all other routes
    if !is_authenticated(&req, &password) {
        return Response::from_html(LOGIN_HTML);
    }

    // Authenticated routes via Router
    Router::new()
        .get_async("/", handle_index)
        .get_async("/api/anime", handle_anime_list)
        .post_async("/api/anime", handle_anime_create)
        .put_async("/api/anime/:id", handle_anime_update)
        .delete_async("/api/anime/:id", handle_anime_delete)
        .or_else_any_method_async("/", |_req, _ctx| async move {
            Response::from_html(LOGIN_HTML)
        })
        .run(req, env)
        .await
}

// ============================================================================
// Scheduled handler (unchanged)
// ============================================================================

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    // 初始化配置
    let config = &Config {
        prowlarr: Prowlarr {
            url: env.var("PROWLARR_URL").unwrap().to_string(),
            api_key: env.var("PROWLARR_API_KEY").unwrap().to_string(),
            indexer: env
                .var("PROWLARR_INDEXER")
                .unwrap()
                .to_string()
                .parse()
                .unwrap(),
        },
        ntfy: Ntfy {
            enable: !env.var("NTFY_TOPIC").unwrap().to_string().trim().is_empty(),
            topic: env.var("NTFY_TOPIC").unwrap().to_string(),
        },
    };

    // 查询所有监听的动画片
    let d1 = env.d1("DB").unwrap();
    let statement = d1.prepare("SELECT * FROM anime");
    let result = statement.all().await.unwrap();
    let animes = &result.results::<Anime>().unwrap();

    // Query existing download records
    let histories = history(&config.prowlarr).await.unwrap();
    let history_urls: HashSet<String> = histories
        .into_iter()
        .filter(|item| item.successful)
        .filter_map(|item| item.data.infoUrl)
        .collect();
    let history_urls = Arc::new(history_urls);

    // 创建异步任务的集合
    let mut tasks = Vec::new();
    for anime in animes {
        let anime = anime.clone();
        let history_urls = history_urls.clone();

        // 为每个 anime 创建一个异步任务并添加到任务集合中
        let task = process_anime(anime, config, history_urls);
        tasks.push(task);
    }

    // 并发执行所有任务
    futures::future::join_all(tasks).await;
}

// ============================================================================
// HTML templates
// ============================================================================

const LOGIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Anime Subscriptions</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#f5f5f5;display:flex;justify-content:center;align-items:center;min-height:100vh}
.card{background:#fff;padding:2rem;border-radius:12px;box-shadow:0 2px 10px rgba(0,0,0,.1);width:100%;max-width:360px}
h2{text-align:center;margin-bottom:1.5rem;color:#333}
input[type=password]{width:100%;padding:12px;border:1px solid #ddd;border-radius:8px;font-size:16px;margin-bottom:1rem}
input:focus{outline:none;border-color:#4a90d9}
button{width:100%;padding:12px;background:#4a90d9;color:#fff;border:none;border-radius:8px;font-size:16px;cursor:pointer}
button:hover{background:#357abd}
.error{color:#e74c3c;text-align:center;margin-top:.5rem;display:none;font-size:14px}
</style>
</head>
<body>
<div class="card">
<h2>🔒 Anime Subscriptions</h2>
<form id="f">
<input type="password" id="p" placeholder="Password" autofocus>
<button type="submit">Login</button>
<p class="error" id="e"></p>
</form>
</div>
<script>
document.getElementById('f').addEventListener('submit',async e=>{
    e.preventDefault();
    const pw=document.getElementById('p').value;
    const errEl=document.getElementById('e');
    errEl.style.display='none';
    try{
        const r=await fetch('/login',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({password:pw}),redirect:'manual'});
        if(r.status===303||r.ok){window.location='/';}
        else{errEl.textContent='密码错误';errEl.style.display='block';}
    }catch(err){errEl.textContent='网络错误';errEl.style.display='block';}
});
</script>
</body>
</html>"#;

const MANAGEMENT_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Anime Subscriptions</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#f5f5f5;color:#333}
header{background:#4a90d9;color:#fff;padding:1rem 1.5rem;display:flex;justify-content:space-between;align-items:center}
header h1{font-size:1.2rem;font-weight:600}
header button{background:rgba(255,255,255,.2);color:#fff;border:1px solid rgba(255,255,255,.3);padding:6px 16px;border-radius:6px;cursor:pointer;font-size:14px}
header button:hover{background:rgba(255,255,255,.3)}
.container{max-width:800px;margin:1.5rem auto;padding:0 1rem}
.toolbar{display:flex;justify-content:flex-end;margin-bottom:1rem}
.btn-add{background:#27ae60;color:#fff;border:none;padding:10px 20px;border-radius:8px;cursor:pointer;font-size:14px}
.btn-add:hover{background:#219a52}
table{width:100%;border-collapse:collapse;background:#fff;border-radius:8px;overflow:hidden;box-shadow:0 1px 5px rgba(0,0,0,.08)}
th,td{padding:12px 16px;text-align:left;border-bottom:1px solid #f0f0f0}
th{background:#fafafa;font-weight:600;font-size:13px;color:#666}
td{font-size:14px}
tr:last-child td{border-bottom:none}
.actions{white-space:nowrap}
.actions button{border:none;padding:5px 12px;border-radius:4px;cursor:pointer;font-size:13px;margin-right:4px}
.btn-edit{background:#eef;color:#4a90d9}
.btn-edit:hover{background:#dde8ff}
.btn-del{background:#fef0f0;color:#e74c3c}
.btn-del:hover{background:#fde}
.overlay{position:fixed;inset:0;background:rgba(0,0,0,.4);display:none;justify-content:center;align-items:center;z-index:100}
.overlay.active{display:flex}
.modal{background:#fff;padding:1.5rem;border-radius:12px;width:90%;max-width:450px;box-shadow:0 4px 20px rgba(0,0,0,.15)}
.modal h3{margin-bottom:1rem;color:#333}
.modal label{display:block;font-size:13px;color:#666;margin:12px 0 4px}
.modal input{width:100%;padding:10px;border:1px solid #ddd;border-radius:6px;font-size:14px}
.modal input:focus{outline:none;border-color:#4a90d9}
.modal-actions{display:flex;justify-content:flex-end;gap:8px;margin-top:1.5rem}
.modal-actions button{padding:8px 20px;border-radius:6px;cursor:pointer;font-size:14px;border:1px solid #ddd}
.btn-cancel{background:#fff;color:#666}
.btn-cancel:hover{background:#f5f5f5}
.btn-save{background:#4a90d9;color:#fff;border-color:#4a90d9}
.btn-save:hover{background:#357abd}
.empty{text-align:center;padding:3rem;color:#999}
</style>
</head>
<body>
<header>
<h1>🎬 Anime Subscriptions</h1>
<button onclick="logout()">Logout</button>
</header>
<div class="container">
<div class="toolbar">
<button class="btn-add" onclick="showModal()">+ Add</button>
</div>
<table>
<thead><tr><th>Keywords</th><th>Exclude</th><th>Indexer</th><th style="width:120px">Actions</th></tr></thead>
<tbody id="tb"></tbody>
</table>
<div id="empty" class="empty" style="display:none">No subscriptions yet</div>
</div>
<div class="overlay" id="ov" onclick="if(event.target===this)hideModal()">
<div class="modal">
<h3 id="mt">Add Subscription</h3>
<input type="hidden" id="eid">
<label>Keywords</label>
<input id="kw" placeholder="e.g. LoliHouse 迷宫饭">
<label>Exclude Keywords</label>
<input id="ek" placeholder="Optional">
<label>Indexer ID</label>
<input id="idx" type="number" placeholder="Optional, uses default if empty">
<div class="modal-actions">
<button class="btn-cancel" onclick="hideModal()">Cancel</button>
<button class="btn-save" onclick="saveAnime()">Save</button>
</div>
</div>
</div>
<script>
const API='/api/anime';
let data=[];
async function load(){
    const r=await fetch(API);
    if(r.status===401||r.status===302){window.location='/';return;}
    const j=await r.json();
    data=j.data||[];render();
}
function render(){
    const tb=document.getElementById('tb');
    const em=document.getElementById('empty');
    if(!data.length){tb.innerHTML='';em.style.display='block';return;}
    em.style.display='none';
    tb.innerHTML=data.map(a=>`<tr>
        <td>${esc(a.keywords)}</td>
        <td>${esc(a.exclude_keywords||'')}</td>
        <td>${a.indexer??'-'}</td>
        <td class="actions">
            <button class="btn-edit" onclick="editAnime(${a.id})">Edit</button>
            <button class="btn-del" onclick="delAnime(${a.id})">Delete</button>
        </td></tr>`).join('');
}
function esc(s){const d=document.createElement('div');d.textContent=s;return d.innerHTML;}
function showModal(id){
    document.getElementById('ov').classList.add('active');
    if(id!=null){
        const a=data.find(x=>x.id===id);
        document.getElementById('mt').textContent='Edit Subscription';
        document.getElementById('eid').value=a.id;
        document.getElementById('kw').value=a.keywords;
        document.getElementById('ek').value=a.exclude_keywords||'';
        document.getElementById('idx').value=a.indexer||'';
    }else{
        document.getElementById('mt').textContent='Add Subscription';
        document.getElementById('eid').value='';
        document.getElementById('kw').value='';
        document.getElementById('ek').value='';
        document.getElementById('idx').value='';
    }
    document.getElementById('kw').focus();
}
function hideModal(){document.getElementById('ov').classList.remove('active');}
async function saveAnime(){
    const id=document.getElementById('eid').value;
    const body={
        keywords:document.getElementById('kw').value.trim(),
        exclude_keywords:document.getElementById('ek').value.trim(),
        indexer:document.getElementById('idx').value?Number(document.getElementById('idx').value):null
    };
    if(!body.keywords){alert('Keywords required');return;}
    const url=id?`${API}/${id}`:API;
    const method=id?'PUT':'POST';
    await fetch(url,{method,headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
    hideModal();load();
}
async function delAnime(id){
    if(!confirm('Delete this subscription?'))return;
    await fetch(`${API}/${id}`,{method:'DELETE'});load();
}
function editAnime(id){showModal(id);}
async function logout(){
    await fetch('/logout',{method:'POST'});window.location='/';
}
load();
</script>
</body>
</html>"#;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cookies_single() {
        let cookies = parse_cookies("session=abc123");
        assert_eq!(cookies, vec![("session", "abc123")]);
    }

    #[test]
    fn test_parse_cookies_multiple() {
        let cookies = parse_cookies("session=abc123; theme=dark; lang=zh");
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0], ("session", "abc123"));
        assert_eq!(cookies[1], ("theme", "dark"));
        assert_eq!(cookies[2], ("lang", "zh"));
    }

    #[test]
    fn test_parse_cookies_empty() {
        let cookies = parse_cookies("");
        assert!(cookies.is_empty());
    }

    #[test]
    fn test_get_session_cookie_found() {
        let cookies = [("session", "mysecret"), ("other", "val")];
        assert_eq!(get_session_cookie(&cookies), Some("mysecret"));
    }

    #[test]
    fn test_get_session_cookie_not_found() {
        let cookies = [("theme", "dark"), ("lang", "zh")];
        assert_eq!(get_session_cookie(&cookies), None);
    }

    #[test]
    fn test_match_exclude_keywords_empty() {
        assert!(!match_exclude_keywords("Some Title", ""));
    }

    #[test]
    fn test_match_exclude_keywords_match() {
        assert!(match_exclude_keywords("LoliHouse 迷宫饭 1080p", "LoliHouse"));
    }

    #[test]
    fn test_match_exclude_keywords_no_match() {
        assert!(!match_exclude_keywords("Some Title", "keyword1 keyword2"));
    }

    #[test]
    fn test_match_exclude_keywords_case_insensitive() {
        assert!(match_exclude_keywords("LOLHouse Title", "lolhouse"));
    }
}
