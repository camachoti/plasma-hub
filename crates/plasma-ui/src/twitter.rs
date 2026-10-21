//! Native Twitter/X bridge for the Rust shell.

use plasma_core::AppEnv;
use plasma_twitter::{NativeMediaInfo, NativeProfileInfo};

#[derive(Clone)]
pub struct TwitterHandle {
    env: AppEnv,
    runtime: tokio::runtime::Handle,
}

impl TwitterHandle {
    pub fn new(env: AppEnv, runtime: tokio::runtime::Handle) -> Self {
        Self { env, runtime }
    }

    pub async fn analyze_tweet(
        &self,
        url: String,
        cookies: Option<String>,
    ) -> Result<NativeMediaInfo, String> {
        let handle = self
            .runtime
            .spawn(async move { plasma_twitter::analyze_twitter_tweet_native(url, cookies).await });
        self.join(handle).await?
    }

    pub async fn analyze_profile(
        &self,
        url: String,
        cookies: Option<String>,
    ) -> Result<NativeProfileInfo, String> {
        let handle = self.runtime.spawn(async move {
            plasma_twitter::analyze_twitter_profile_native(url, cookies).await
        });
        self.join(handle).await?
    }

    pub async fn download_tweet(
        &self,
        id: String,
        url: String,
        filename: String,
        cookies: Option<String>,
    ) -> Result<(), String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_twitter::download_twitter_native(&env, id, url, filename, cookies).await
        });
        self.join(handle).await?
    }

    pub async fn download_profile(
        &self,
        id: String,
        username: String,
        media_urls: Vec<String>,
        cookies: Option<String>,
    ) -> Result<(), String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_twitter::download_twitter_profile_native(&env, id, username, media_urls, cookies)
                .await
        });
        self.join(handle).await?
    }

    pub async fn load_cookies(&self) -> Result<String, String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || plasma_twitter::load_twitter_cookies(&env)).await
        });
        self.join(handle)
            .await?
            .map_err(|error| error.to_string())?
    }

    pub async fn save_cookies(&self, cookies: String) -> Result<(), String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                plasma_twitter::save_twitter_cookies(&env, &cookies)
            })
            .await
        });
        self.join(handle)
            .await?
            .map_err(|error| error.to_string())?
    }

    pub fn download_dir(&self) -> std::path::PathBuf {
        self.env
            .paths()
            .download_dir()
            .unwrap_or_else(|_| self.env.paths().data_dir().join("downloads"))
    }

    async fn join<T>(&self, handle: tokio::task::JoinHandle<T>) -> Result<T, String> {
        handle
            .await
            .map_err(|error| format!("Tarefa do Twitter falhou: {error}"))
    }
}
