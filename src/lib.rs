use std::collections::HashSet;

use reqwest::header;
use serde::{Deserialize, Serialize};
use worker::wasm_bindgen::JsValue;
use worker::{
    event, Env, Request, Response, RouteContext, Router, ScheduleContext, ScheduledEvent,
};

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

const NTFY_PROXY_URL: &str = "https://ntfy.zhu327.deno.net";

async fn send_message(ntfy: &Ntfy, message: &str) -> std::result::Result<(), reqwest::Error> {
    let url = format!("{}/{}", NTFY_PROXY_URL, ntfy.topic);

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
    exclude_keywords
        .split_whitespace()
        .any(|keyword| title.to_lowercase().contains(&keyword.to_lowercase()))
}

async fn process_anime(
    anime: Anime,
    config: &Config,
    history_urls: &HashSet<String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let items = search(&config.prowlarr, anime.indexer, &anime.keywords).await?;
    for item in items {
        if item.age > 2 || match_exclude_keywords(&item.title, &anime.exclude_keywords) {
            continue;
        }
        if history_urls.contains(&item.infoUrl) {
            continue;
        }
        download(&config.prowlarr, anime.indexer, &item.guid).await?;
        if config.ntfy.enable {
            send_message(&config.ntfy, &format!("Downloading {}", item.title)).await?;
        }
        break;
    }
    Ok(())
}

// ============================================================================
// Response helpers
// ============================================================================

fn json_ok(data: &impl Serialize) -> worker::Result<Response> {
    Response::from_json(data)
}

fn json_err(msg: &str, status: u16) -> worker::Result<Response> {
    Response::from_json(&serde_json::json!({"ok": false, "error": msg}))
        .map(|r| r.with_status(status))
}

fn parse_id(ctx: &RouteContext<()>) -> worker::Result<i32> {
    ctx.param("id")
        .ok_or_else(|| worker::Error::RustError("missing id".into()))?
        .parse()
        .map_err(|_| worker::Error::RustError("invalid id".into()))
}

// ============================================================================
// Request types
// ============================================================================

#[derive(Debug, Deserialize)]
struct AnimeInput {
    keywords: String,
    exclude_keywords: Option<String>,
    indexer: Option<u32>,
}

// ============================================================================
// Route handlers
// ============================================================================

async fn handle_index(_req: Request, _ctx: RouteContext<()>) -> worker::Result<Response> {
    Response::from_html(MANAGEMENT_HTML)
}

async fn handle_anime_list(_req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let d1 = ctx.d1("DB")?;
    let animes: Vec<Anime> = d1
        .prepare("SELECT * FROM anime")
        .all()
        .await?
        .results::<Anime>()?;
    json_ok(&serde_json::json!({"ok": true, "data": animes}))
}

fn anime_params(input: &AnimeInput) -> [JsValue; 3] {
    [
        JsValue::from_str(&input.keywords),
        JsValue::from_str(input.exclude_keywords.as_deref().unwrap_or_default()),
        input
            .indexer
            .map(|i| JsValue::from_f64(i as f64))
            .unwrap_or(JsValue::null()),
    ]
}

async fn handle_anime_create(mut req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let input: AnimeInput = req.json().await?;
    if input.keywords.trim().is_empty() {
        return json_err("keywords不能为空", 400);
    }

    let d1 = ctx.d1("DB")?;
    d1.prepare("INSERT INTO anime (keywords, exclude_keywords, indexer) VALUES (?, ?, ?)")
        .bind(&anime_params(&input))?
        .run()
        .await?;

    let anime = d1
        .prepare("SELECT * FROM anime WHERE id = last_insert_rowid()")
        .first::<Anime>(None)
        .await?;
    json_ok(&serde_json::json!({"ok": true, "data": anime}))
}

async fn handle_anime_update(mut req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let id = parse_id(&ctx)?;
    let input: AnimeInput = req.json().await?;
    if input.keywords.trim().is_empty() {
        return json_err("keywords不能为空", 400);
    }

    let d1 = ctx.d1("DB")?;
    let mut params: Vec<JsValue> = anime_params(&input).into();
    params.push(JsValue::from_f64(id as f64));

    d1.prepare("UPDATE anime SET keywords = ?, exclude_keywords = ?, indexer = ? WHERE id = ?")
        .bind(&params)?
        .run()
        .await?;

    let anime = d1
        .prepare("SELECT * FROM anime WHERE id = ?")
        .bind(&[JsValue::from_f64(id as f64)])?
        .first::<Anime>(None)
        .await?;
    json_ok(&serde_json::json!({"ok": true, "data": anime}))
}

async fn handle_anime_delete(_req: Request, ctx: RouteContext<()>) -> worker::Result<Response> {
    let id = parse_id(&ctx)?;
    let d1 = ctx.d1("DB")?;
    d1.prepare("DELETE FROM anime WHERE id = ?")
        .bind(&[JsValue::from_f64(id as f64)])?
        .run()
        .await?;
    json_ok(&serde_json::json!({"ok": true}))
}

// ============================================================================
// Fetch handler
// ============================================================================

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: worker::Context) -> worker::Result<Response> {
    Router::new()
        .get_async("/", handle_index)
        .get_async("/api/anime", handle_anime_list)
        .post_async("/api/anime", handle_anime_create)
        .put_async("/api/anime/:id", handle_anime_update)
        .delete_async("/api/anime/:id", handle_anime_delete)
        .or_else_any_method_async("/", |_req, _ctx| async move {
            Response::error("Not Found", 404)
        })
        .run(req, env)
        .await
}

// ============================================================================
// Scheduled handler (unchanged)
// ============================================================================

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let config = Config {
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

    let d1 = env.d1("DB").unwrap();
    let animes: Vec<Anime> = d1
        .prepare("SELECT * FROM anime")
        .all()
        .await
        .unwrap()
        .results()
        .unwrap();

    let histories = history(&config.prowlarr).await.unwrap();
    let history_urls: HashSet<String> = histories
        .into_iter()
        .filter(|item| item.successful)
        .filter_map(|item| item.data.infoUrl)
        .collect();

    let tasks: Vec<_> = animes
        .into_iter()
        .map(|anime| process_anime(anime, &config, &history_urls))
        .collect();
    futures::future::join_all(tasks).await;
}

// ============================================================================
// HTML templates
// ============================================================================

const MANAGEMENT_HTML: &str = include_str!("templates/management.html");

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_exclude_keywords_empty() {
        assert!(!match_exclude_keywords("Some Title", ""));
    }

    #[test]
    fn test_match_exclude_keywords_match() {
        assert!(match_exclude_keywords(
            "LoliHouse 迷宫饭 1080p",
            "LoliHouse"
        ));
    }

    #[test]
    fn test_match_exclude_keywords_no_match() {
        assert!(!match_exclude_keywords("Some Title", "keyword1 keyword2"));
    }

    #[test]
    fn test_match_exclude_keywords_case_insensitive() {
        assert!(match_exclude_keywords("LOLHouse Title", "lolhouse"));
    }

    #[test]
    fn test_management_html_has_mobile_breakpoint() {
        assert!(MANAGEMENT_HTML.contains("@media (max-width:600px)"));
    }

    #[test]
    fn test_management_html_renders_mobile_table_labels() {
        assert!(MANAGEMENT_HTML.contains("data-label=\"Keywords\""));
        assert!(MANAGEMENT_HTML.contains("data-label=\"Exclude\""));
        assert!(MANAGEMENT_HTML.contains("data-label=\"Indexer\""));
        assert!(MANAGEMENT_HTML.contains("data-label=\"Actions\""));
    }
}
