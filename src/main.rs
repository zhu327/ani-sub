use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use reqwest::blocking::Client;

#[derive(Debug, Deserialize)]
struct Config {
    prowlarr: Prowlarr,
    ntfy: Ntfy,
    animes: Vec<Anime>,
}

#[derive(Debug, Deserialize)]
struct Prowlarr {
    url: String,
    api_key: String,
    indexer: u32,
}

#[derive(Debug, Deserialize)]
struct Ntfy {
    enable: bool,
    topic: String,
}

#[derive(Debug, Deserialize)]
struct Anime {
    keywords: String,
    exclude_keywords: String,
}

fn read_config_file(file_path: &PathBuf) -> Result<Config, Box<dyn std::error::Error>> {
    // 读取配置文件内容
    let config_content = std::fs::read_to_string(file_path)?;

    // 解析配置文件内容为结构体
    let config: Config = serde_yaml::from_str(&config_content)?;

    Ok(config)
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    age: u32,
    title: String,
    guid: String,
}

fn search(prowlarr: &Prowlarr, keywords: &str) -> Result<Vec<SearchResult>, reqwest::Error> {
    let url = format!("{}/api/v1/search", prowlarr.url);

    let params = [
        ("query", keywords),
        ("indexerIds", &prowlarr.indexer.to_string()),
    ];

    let client = Client::new();
    let response = client
        .get(&url)
        .query(&params)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("X-Api-Key", &prowlarr.api_key)
        .send()?;

    response.error_for_status_ref()?;

    let result: Vec<SearchResult> = response.json()?;
    Ok(result)
}

#[derive(Debug, Deserialize)]
struct HistoryData {
    url: String,
}

#[derive(Debug, Deserialize)]
struct HistoryResult {
    data: HistoryData,
    successful: bool,
}

fn history(prowlarr: &Prowlarr) -> Result<Vec<HistoryResult>, reqwest::Error> {
    let url = format!("{}/api/v1/history/indexer", prowlarr.url);

    let params = [
        ("indexerId", &prowlarr.indexer.to_string()),
        ("eventType", &"releaseGrabbed".to_string()),
        ("limit", &"100".to_string()),
    ];

    let client = Client::new();
    let result: Vec<HistoryResult> = client
        .get(&url)
        .query(&params)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("X-Api-Key", &prowlarr.api_key)
        .send()?
        .error_for_status()?
        .json()?;

    Ok(result)
}

#[allow(warnings)]
#[warn(non_snake_case)]
#[derive(Debug, Deserialize, Serialize)]
struct DownloadRequest {
    guid: String,
    indexerId: u32,
}

fn download(prowlarr: &Prowlarr, guid: &str) -> Result<(), reqwest::Error> {
    let url = format!("{}/api/v1/search", prowlarr.url);

    let request_body = DownloadRequest {
        guid: guid.to_string(),
        indexerId: prowlarr.indexer,
    };

    let client = Client::new();
    let response = client
        .post(&url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("X-Api-Key", &prowlarr.api_key)
        .json(&request_body)
        .send()?;

    response.error_for_status_ref()?;

    Ok(())
}

fn send_message(ntfy: &Ntfy, message: &str) -> Result<(), reqwest::Error> {
    let url = format!("https://ntfy.sh/{}", ntfy.topic);

    let client = Client::new();
    let response = client
        .post(&url)
        .body(message.to_string())
        .send()?;

    response.error_for_status_ref()?;

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

#[derive(Debug, StructOpt)]
struct Cli {
    #[structopt(long, parse(from_os_str))]
    config: std::path::PathBuf,
}

fn main() {
    // 从命令行参数解析配置文件路径
    let args = Cli::from_args();

    // Load configuration file
    let config = read_config_file(&args.config).unwrap();

    // Query existing download records
    let histories = history(&config.prowlarr).unwrap();
    let history_urls: HashSet<String> = histories
        .into_iter()
        .filter(|item| item.successful)
        .map(|item| item.data.url)
        .collect();

    for anime in &config.animes {
        let items = search(&config.prowlarr, &anime.keywords).unwrap();
        for item in items {
            if item.age > 2 || match_exclude_keywords(&item.title, &anime.exclude_keywords) {
                continue;
            }

            // Check if already downloaded
            if history_urls.contains(&item.guid) {
                continue;
            }

            // Download
            download(&config.prowlarr, &item.guid).unwrap();

            // Notify
            if config.ntfy.enable {
                send_message(&config.ntfy, &format!("Downloading {}", item.title)).unwrap();
            }

            break;
        }
    }
}
