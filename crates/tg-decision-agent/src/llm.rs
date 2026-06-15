use std::collections::VecDeque;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, system: &str, user: &str, json_schema: Option<&Value>) -> Result<String>;

    async fn probe(&self) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
}

impl OpenAiCompatibleConfig {
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("TG_LLM_BASE_URL")
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .map_err(|_| anyhow!("TG_LLM_BASE_URL or OPENAI_BASE_URL is required"))?;
        let api_key = std::env::var("TG_LLM_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| anyhow!("TG_LLM_API_KEY or OPENAI_API_KEY is required"))?;
        let model = std::env::var("TG_LLM_MODEL")
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "gpt-4o-mini".to_owned());
        let timeout_secs = std::env::var("TG_LLM_TIMEOUT_SECS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(15);

        Ok(Self {
            base_url,
            api_key,
            model,
            timeout: Duration::from_secs(timeout_secs),
        })
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleClient {
    http: reqwest::Client,
    config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleClient {
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let auth = format!("Bearer {}", config.api_key);
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&auth)?);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(config.timeout)
            .build()?;
        Ok(Self { http, config })
    }

    fn completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn chat(&self, system: &str, user: &str, json_schema: Option<&Value>) -> Result<String> {
        let mut body = json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        });

        if let Some(schema) = json_schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "trade_decision",
                    "strict": true,
                    "schema": schema
                }
            });
        }

        let response = self
            .http
            .post(self.completions_url())
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let completion: ChatCompletionResponse = response.json().await?;
        completion
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| anyhow!("LLM response did not contain choices[0].message.content"))
    }

    async fn probe(&self) -> Result<()> {
        self.chat(
            "Health check. Reply with compact JSON only.",
            r#"{"ping":"ok"}"#,
            None,
        )
        .await
        .map(|_| ())
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug)]
pub struct MockLlmClient {
    responses: Mutex<VecDeque<Result<String, String>>>,
    probe_result: Mutex<Result<(), String>>,
}

impl MockLlmClient {
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            probe_result: Mutex::new(Ok(())),
        }
    }

    pub fn with_probe_error(message: impl Into<String>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::new()),
            probe_result: Mutex::new(Err(message.into())),
        }
    }

    pub async fn push_response(&self, response: impl Into<String>) {
        self.responses.lock().await.push_back(Ok(response.into()));
    }

    pub async fn set_probe_result(&self, result: Result<(), String>) {
        *self.probe_result.lock().await = result;
    }
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn chat(
        &self,
        _system: &str,
        _user: &str,
        _json_schema: Option<&Value>,
    ) -> Result<String> {
        let mut responses = self.responses.lock().await;
        match responses.pop_front() {
            Some(Ok(response)) => Ok(response),
            Some(Err(message)) => Err(anyhow!(message)),
            None => Err(anyhow!("mock LLM response queue is empty")),
        }
    }

    async fn probe(&self) -> Result<()> {
        self.probe_result
            .lock()
            .await
            .clone()
            .map_err(|message| anyhow!(message))
    }
}

#[cfg(test)]
mod tests {
    use super::{LlmClient, OpenAiCompatibleClient, OpenAiCompatibleConfig};

    #[tokio::test]
    #[ignore = "requires TG_LLM_BASE_URL/TG_LLM_API_KEY/TG_LLM_MODEL and network"]
    async fn real_llm_smoke_probe() {
        let config = OpenAiCompatibleConfig::from_env().expect("LLM env");
        let client = OpenAiCompatibleClient::new(config).expect("client");
        client.probe().await.expect("probe");
    }
}
