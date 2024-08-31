use std::collections::HashSet;
use std::sync::Arc;

use reqwest::header;
use serde::{Deserialize, Serialize};
use worker::{event, Env, ScheduleContext, ScheduledEvent};

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

#[derive(Debug, Deserialize, Clone)]
struct Anime {
    keywords: String,
    exclude_keywords: String,
    indexer: u32,
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
    mut indexer: u32,
    keywords: &str,
) -> Result<Vec<SearchResult>, reqwest::Error> {
    let url = format!("{}/api/v1/search", prowlarr.url);

    if indexer == 0 {
        indexer = prowlarr.indexer
    }

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
    infoUrl: String,
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

async fn history(prowlarr: &Prowlarr) -> Result<Vec<HistoryItem>, reqwest::Error> {
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

async fn download(prowlarr: &Prowlarr, mut indexer: u32, guid: &str) -> Result<(), reqwest::Error> {
    let url = format!("{}/api/v1/search", prowlarr.url);

    if indexer == 0 {
        indexer = prowlarr.indexer
    }

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

async fn send_message(ntfy: &Ntfy, message: &str) -> Result<(), reqwest::Error> {
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
    config: Arc<&Config>,
    history_urls: Arc<HashSet<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let config = Arc::new(config);

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
        .map(|item| item.data.infoUrl)
        .collect();
    let history_urls = Arc::new(history_urls);

    // 创建异步任务的集合
    let mut tasks = Vec::new();
    for anime in animes {
        let anime = anime.clone();
        let config = config.clone();
        let history_urls = history_urls.clone();

        // 为每个 anime 创建一个异步任务并添加到任务集合中
        let task = process_anime(anime, config, history_urls);
        tasks.push(task);
    }

    // 并发执行所有任务
    futures::future::join_all(tasks).await;
}
