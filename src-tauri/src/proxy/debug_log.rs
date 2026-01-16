use std::fs::OpenOptions;
use std::io::Write;
use serde_json::Value;

/// 写入日志文件
pub fn write_log_entry(entry: String) {
    if let Some(home) = dirs::home_dir() {
        let log_dir = home.join("tmp").join("log");
        if let Err(e) = std::fs::create_dir_all(&log_dir) {
            log::error!("Failed to create log dir: {}", e);
            return;
        }

        let now = chrono::Local::now();
        let filename = format!("cc-{}.log", now.format("%Y%m%d%H"));
        let log_path = log_dir.join(filename);

        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            Ok(f) => f,
            Err(e) => {
                log::error!("Failed to open log file: {}", e);
                return;
            }
        };

        if let Err(e) = file.write_all(entry.as_bytes()) {
            log::error!("Failed to write to log file: {}", e);
        }
    }
}

/// 记录请求日志
pub fn log_request(
    request_id: &str,
    provider_name: &str,
    url: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) {
    let now = chrono::Local::now();
    let entry = format!(
        "[{}] [REQ:{}] Provider: {}\nURL: {}\nHeaders: {:?}\nBody: {}\n\n--------------------------------------------------\n\n",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        request_id,
        provider_name,
        url,
        headers,
        serde_json::to_string_pretty(body).unwrap_or_else(|_| "Invalid JSON".to_string())
    );
    write_log_entry(entry);
}

/// 记录响应头日志
pub fn log_response_headers(
    request_id: &str,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
) {
    let now = chrono::Local::now();
    let entry = format!(
        "[{}] [RES:{}] Status: {}\nHeaders: {:?}\n\n--------------------------------------------------\n\n",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        request_id,
        status,
        headers
    );
    write_log_entry(entry);
}

/// 记录响应内容块（用于流式）
pub fn log_response_chunk(request_id: &str, chunk: &str) {
    let now = chrono::Local::now();
    let entry = format!(
        "[{}] [CHUNK:{}] Content: {}\n",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        request_id,
        chunk
    );
    write_log_entry(entry);
}

/// 记录响应错误日志
pub fn log_response_error(request_id: &str, status: u16, body: &Option<String>) {
    let now = chrono::Local::now();
    let entry = format!(
        "[{}] [ERR:{}] Upstream Error Status: {}\nBody: {}\n\n--------------------------------------------------\n\n",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        request_id,
        status,
        body.as_deref().unwrap_or("(empty)")
    );
    write_log_entry(entry);
}

/// 记录网络错误日志
pub fn log_network_error(request_id: &str, error: &str) {
    let now = chrono::Local::now();
    let entry = format!(
        "[{}] [NET_ERR:{}] Error: {}\n\n--------------------------------------------------\n\n",
        now.format("%Y-%m-%d %H:%M:%S%.3f"),
        request_id,
        error
    );
    write_log_entry(entry);
}

/// 用于在 Response Extensions 中存储 Request ID
#[derive(Clone)]
pub struct LogRequestId(pub String);