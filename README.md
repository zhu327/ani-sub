# ani-sub

ani-sub is an open-source tool designed to subscribe to anime resources from [Mikan](https://mikanani.me). It uses a configuration file to manage subscriptions and notifications.

## Features

- Query and download anime episodes from Mikan using Prowlarr.
- Customize notifications with ntfy.
- Flexible configuration for multiple anime subscriptions.

## Installation

1. Clone the repository:

   ```bash
   git clone https://github.com/zhu327/ani-sub.git
   cd ani-sub
   ```

2. Build the project:

   ```bash
   cargo build --release
   ```

3. Move the binary to /usr/local/bin:

   ```bash
   mv target/release/ani-sub /usr/local/bin
   ```

4. Run ani-sub:

   ```bash
   ani-sub --config path/to/config.yaml
   ```

## Configuration

ani-sub uses a YAML configuration file to manage subscriptions. Here's an example configuration file:

```yaml
prowlarr:
  url: "your_prowlarr_url"
  api_key: "your_prowlarr_api_key"
  indexer: 6 # your prowlarr mikan index number

ntfy:
  enable: true
  topic: "your_ntfy_topic"

animes:
  - keywords: "LoliHouse 迷宫饭"
    exclude_keywords: ""
  - keywords: "桜都字幕组 葬送的芙莉莲 简体内嵌"
    exclude_keywords: ""
```

Adjust the values based on your preferences and subscriptions.

## Usage

Run ani-sub with the path to your configuration file:

```bash
ani-sub --config path/to/config.yaml
```

### Scheduling with crontab

You can schedule ani-sub to run periodically using `crontab`. Open your crontab configuration:

```bash
crontab -e
```

Add the following line to run ani-sub every hour:

```bash
0 * * * * /usr/local/bin/ani-sub --config path/to/config.yaml
```

This will execute ani-sub every hour and check for new episodes based on your subscriptions.

## License

This project is licensed under the [MIT License](LICENSE).
