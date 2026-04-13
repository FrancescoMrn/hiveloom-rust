use anyhow::{Context, Result};
use reqwest::Client;
use serde::de::DeserializeOwned;

/// Shared HTTP client for CLI subcommands talking to the admin API.
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

impl ApiClient {
    pub fn new(endpoint: Option<String>, token: Option<String>) -> Self {
        let base_url = endpoint.unwrap_or_else(|| "http://127.0.0.1:3000".to_string());
        Self {
            client: Client::new(),
            base_url,
            token,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref tok) = self.token {
            builder.bearer_auth(tok)
        } else {
            builder
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let req = self.apply_auth(self.client.get(self.url(path)));
        let resp = req.send().await.context("API request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error ({}): {}", status, body);
        }
        resp.json::<T>().await.context("failed to parse response")
    }

    pub async fn post<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let req = self.apply_auth(self.client.post(self.url(path))).json(body);
        let resp = req.send().await.context("API request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error ({}): {}", status, body);
        }
        resp.json::<T>().await.context("failed to parse response")
    }

    pub async fn put<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let req = self.apply_auth(self.client.put(self.url(path))).json(body);
        let resp = req.send().await.context("API request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error ({}): {}", status, body);
        }
        resp.json::<T>().await.context("failed to parse response")
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        let req = self.apply_auth(self.client.delete(self.url(path)));
        let resp = req.send().await.context("API request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error ({}): {}", status, body);
        }
        Ok(())
    }
}
