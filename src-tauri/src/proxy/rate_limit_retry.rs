//! Rate Limit 重试机制
//!
//! 当检测到 "Rate limit error" 时，自动进行指数退避重试

use crate::proxy::ProxyError;
use std::time::Duration;
use tokio::time::sleep;

/// 重试配置
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_retries: usize,
    /// 初始退避时间（秒）
    pub initial_backoff_seconds: f64,
    /// 退避倍数
    pub backoff_multiplier: f64,
    /// 最大退避时间（秒）
    pub max_backoff_seconds: f64,
    /// 抖动因子（0.0-1.0，用于避免惊群效应）
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,                    // 最多重试3次
            initial_backoff_seconds: 1.0,     // 初始等待1秒
            backoff_multiplier: 2.0,           // 每次翻倍
            max_backoff_seconds: 30.0,         // 最多等待30秒
            jitter_factor: 0.1,                // 10%的抖动
        }
    }
}

/// 重试状态
#[derive(Debug)]
pub struct RetryState {
    pub attempt: usize,
    pub config: RetryConfig,
}

impl RetryState {
    /// 创建新的重试状态
    pub fn new(config: RetryConfig) -> Self {
        Self { attempt: 0, config }
    }

    /// 检查是否可以重试
    pub fn can_retry(&self) -> bool {
        self.attempt < self.config.max_retries
    }

    /// 计算下次重试的等待时间
    pub fn calculate_backoff(&self) -> Duration {
        let base_delay = self.config.initial_backoff_seconds
            * self.config.backoff_multiplier.powi(self.attempt as i32);

        // 限制最大等待时间
        let capped_delay = base_delay.min(self.config.max_backoff_seconds);

        // 添加抖动避免惊群效应
        let jitter = capped_delay * self.config.jitter_factor * (rand::random::<f64>() - 0.5);
        let final_delay = (capped_delay + jitter).max(0.0);

        Duration::from_secs_f64(final_delay)
    }

    /// 执行等待并增加重试计数
    pub async fn wait_and_increment(&mut self) {
        let delay = self.calculate_backoff();
        log::info!(
            "[RETRY] 检测到 Rate limit error，等待 {:.1} 秒后重试 (第 {}/{} 次)",
            delay.as_secs_f64(),
            self.attempt + 1,
            self.config.max_retries
        );

        sleep(delay).await;
        self.attempt += 1;
    }
}

/// 检测是否为 Rate limit 错误
pub fn is_rate_limit_error(content: &str) -> bool {
    content.to_lowercase().contains("rate limit")
}

/// 检测 SSE 流中的 Rate limit 错误
///
/// 从 SSE 数据中解析错误消息，检查是否包含 rate limit 相关内容
pub fn detect_rate_limit_in_sse(sse_data: &str) -> bool {
    // 解析 SSE 数据，查找 data 行
    for line in sse_data.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            // 跳过 [DONE] 标记
            if data.trim() == "[DONE]" {
                continue;
            }

            // 尝试解析为 JSON
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(data) {
                // 检查各种可能包含错误信息的字段
                if let Some(error_text) = extract_error_from_json(&json_value) {
                    if is_rate_limit_error(&error_text) {
                        return true;
                    }
                }
            } else {
                // 如果不是 JSON，直接检查文本内容
                if is_rate_limit_error(data) {
                    return true;
                }
            }
        }
    }
    false
}

/// 从 JSON 中提取错误信息
fn extract_error_from_json(json: &serde_json::Value) -> Option<String> {
    // 检查常见的错误字段
    if let Some(error) = json.get("error") {
        // error 可能是字符串或对象
        if let Some(error_str) = error.as_str() {
            return Some(error_str.to_string());
        } else if let Some(error_obj) = error.as_object() {
            // 检查 error.message, error.detail 等字段
            if let Some(message) = error_obj.get("message").and_then(|v| v.as_str()) {
                return Some(message.to_string());
            }
            if let Some(detail) = error_obj.get("detail").and_then(|v| v.as_str()) {
                return Some(detail.to_string());
            }
        }
    }

    // 检查 content_block_delta 中的 text (Claude API)
    if let Some(delta) = json.get("delta") {
        if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
            return Some(text.to_string());
        }
    }

    // 检查 message 字段
    if let Some(message) = json.get("message").and_then(|v| v.as_str()) {
        return Some(message.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_rate_limit_error() {
        assert!(is_rate_limit_error("Rate limit error, please wait"));
        assert!(is_rate_limit_error("RATE LIMIT EXCEEDED"));
        assert!(is_rate_limit_error("You have exceeded the rate limit"));
        assert!(!is_rate_limit_error("Internal server error"));
        assert!(!is_rate_limit_error("Authentication failed"));
    }

    #[test]
    fn test_detect_rate_limit_in_sse() {
        let sse_data = r#"event: message_start
data: {"type":"message_start","message":{"role":"assistant","stop_sequence":null,"usage":{"output_tokens":0,"cache_creation_input_tokens":0,"input_tokens":0,"cache_read_input_tokens":0},"stop_reason":null,"model":"error","id":"msg_e76873af-d47","type":"message","content":[]}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"text":"","type":"text"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"text":"Rate limit error, please wait before trying again","type":"text_delta"}}
"#;

        assert!(detect_rate_limit_in_sse(sse_data));
    }

    #[test]
    fn test_detect_rate_limit_in_sse_no_error() {
        let sse_data = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"text":"Hello, how can I help you?","type":"text_delta"}}
"#;

        assert!(!detect_rate_limit_in_sse(sse_data));
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_backoff_seconds, 1.0);
        assert_eq!(config.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_retry_state_can_retry() {
        let config = RetryConfig::default();
        let mut state = RetryState::new(config);

        assert!(state.can_retry()); // 第0次，可以重试

        state.attempt = 1;
        assert!(state.can_retry()); // 第1次，可以重试

        state.attempt = 2;
        assert!(state.can_retry()); // 第2次，可以重试

        state.attempt = 3;
        assert!(!state.can_retry()); // 第3次，超过最大重试次数
    }

    #[test]
    fn test_calculate_backoff() {
        let config = RetryConfig {
            max_retries: 3,
            initial_backoff_seconds: 1.0,
            backoff_multiplier: 2.0,
            max_backoff_seconds: 10.0,
            jitter_factor: 0.0, // 禁用抖动以便测试
        };

        let state = RetryState::new(config);

        // 第0次重试：1.0 * 2^0 = 1.0秒
        let delay0 = state.calculate_backoff();
        assert_eq!(delay0.as_secs_f64(), 1.0);

        // 第1次重试：1.0 * 2^1 = 2.0秒
        let mut state1 = state.clone();
        state1.attempt = 1;
        let delay1 = state1.calculate_backoff();
        assert_eq!(delay1.as_secs_f64(), 2.0);

        // 第2次重试：1.0 * 2^2 = 4.0秒
        let mut state2 = state.clone();
        state2.attempt = 2;
        let delay2 = state2.calculate_backoff();
        assert_eq!(delay2.as_secs_f64(), 4.0);
    }

    #[test]
    fn test_calculate_backoff_max_cap() {
        let config = RetryConfig {
            max_retries: 10,
            initial_backoff_seconds: 1.0,
            backoff_multiplier: 2.0,
            max_backoff_seconds: 5.0, // 最大5秒
            jitter_factor: 0.0,
        };

        let mut state = RetryState::new(config);
        state.attempt = 10; // 很大的重试次数

        let delay = state.calculate_backoff();
        assert_eq!(delay.as_secs_f64(), 5.0); // 应该被限制在5秒
    }
}