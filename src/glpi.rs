use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, LOCATION};
use serde::Deserialize;
use std::collections::HashMap;

/// Thin client for GLPI REST API endpoints we need.
#[derive(Debug, Clone)]
pub struct GlpiClient {
    base_url: String,
    app_token: Option<String>,
    user_token: String,
    http: reqwest::Client,
    session_token: Option<String>,
}

/// Minimal ticket surface used by the notifier.
#[derive(Debug, Clone)]
pub struct Ticket {
    pub id: i64,
    pub name: String,
    pub requester: Option<String>,
}

#[derive(Deserialize)]
struct InitSessionResp {
    session_token: String,
}

impl GlpiClient {
    pub async fn new(
        base_url: String,
        app_token: Option<String>,
        user_token: String,
        verify_ssl: bool,
    ) -> Result<Self> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .danger_accept_invalid_certs(!verify_ssl)
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none()) // we handle 30x manually
            .build()?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            app_token,
            user_token,
            http: client,
            session_token: None,
        })
    }

    fn hdrs(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("Accept", HeaderValue::from_static("application/json"));
        h.insert("User-Agent", HeaderValue::from_static("glpi-notifier-rs/0.1"));
        if let Some(ref s) = self.session_token {
            h.insert("Session-Token", HeaderValue::from_str(s).unwrap());
        }
        if let Some(ref a) = self.app_token {
            h.insert("App-Token", HeaderValue::from_str(a).unwrap());
        }
        h
    }

    /// Authenticate (initSession). Also follows simple 30x to a new base URL if needed.
    pub async fn init_session(&mut self) -> Result<()> {
        let mut hdrs = HeaderMap::new();
        hdrs.insert("Accept", HeaderValue::from_static("application/json"));
        hdrs.insert("User-Agent", HeaderValue::from_static("glpi-notifier-rs/0.1"));
        hdrs.insert(
            "Authorization",
            HeaderValue::from_str(&format!("user_token {}", self.user_token))?,
        );
        if let Some(ref a) = self.app_token {
            hdrs.insert("App-Token", HeaderValue::from_str(a)?);
        }

        let url = format!("{}/initSession", self.base_url.trim_end_matches('/'));
        let mut r = self.http.get(&url).headers(hdrs.clone()).send().await?;

        if r.status().is_redirection() {
            if let Some(loc) = r.headers().get(LOCATION).and_then(|v| v.to_str().ok()) {
                let new_base = loc.trim_end_matches('/').trim_end_matches("/initSession");
                self.base_url = new_base.to_string();
                let url2 = format!("{}/initSession", self.base_url);
                r = self.http.get(&url2).headers(hdrs.clone()).send().await?;
            }
        }

        if !r.status().is_success() {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            return Err(anyhow!("initSession failed: {status} | body: {body}"));
        }

        let data: InitSessionResp = r.json().await?;
        self.session_token = Some(data.session_token);
        Ok(())
    }

    pub async fn kill_session(&mut self) -> Result<()> {
        if self.session_token.is_none() {
            return Ok(());
        }
        let url = format!("{}/killSession", self.base_url);
        let _ = self.http.get(url).headers(self.hdrs()).send().await?;
        self.session_token = None;
        Ok(())
    }

    async fn ensure_session(&mut self) -> Result<()> {
        if self.session_token.is_none() {
            self.init_session().await?;
        }
        Ok(())
    }

    /// /listSearchOptions/Ticket – map UID -> numeric field id
    pub async fn list_search_options(&mut self, itemtype: &str) -> Result<serde_json::Value> {
        self.ensure_session().await?;
        let url = format!("{}/listSearchOptions/{}", self.base_url, itemtype);
        let r = self.http.get(url).headers(self.hdrs()).send().await?;
        if !r.status().is_success() {
            return Err(anyhow!("listSearchOptions failed: {}", r.status()));
        }
        Ok(r.json().await?)
    }

    pub async fn resolve_field_ids(&mut self, uids: &[&str]) -> Result<HashMap<String, i64>> {
        let opts = self.list_search_options("Ticket").await?;
        let mut map = HashMap::new();
        if let Some(obj) = opts.as_object() {
            for (k, v) in obj {
                if let (Ok(id_num), Some(uid)) = (k.parse::<i64>(), v.get("uid")) {
                    if let Some(uid_s) = uid.as_str() {
                        if uids.contains(&uid_s) {
                            map.insert(uid_s.to_string(), id_num);
                        }
                    }
                }
            }
        }
        Ok(map)
    }

    /// Search tickets with status=New. Optionally include requester field.
    pub async fn search_new_tickets(
        &mut self,
        id_field: i64,
        name_field: i64,
        status_field: i64,
        requester_field: Option<i64>,
        max_rows: usize,
    ) -> Result<Vec<Ticket>> {
        self.ensure_session().await?;

        let mut params: Vec<(&str, String)> = vec![
            ("criteria[0][field]", status_field.to_string()),
            ("criteria[0][searchtype]", "equals".into()),
            ("criteria[0][value]", "1".into()), // 1 = New
            ("sort", id_field.to_string()),
            ("order", "DESC".into()),
            ("range", format!("0-{}", max_rows)),
            ("forcedisplay[0]", id_field.to_string()),
            ("forcedisplay[1]", name_field.to_string()),
            ("forcedisplay[2]", status_field.to_string()),
        ];

        if let Some(req) = requester_field {
            params.push(("forcedisplay[3]", req.to_string()));
        }

        let url = format!("{}/search/Ticket", self.base_url);
        let r = self
            .http
            .get(url)
            .headers(self.hdrs())
            .query(&params)
            .send()
            .await?;

        if !r.status().is_success() {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            return Err(anyhow!("search/Ticket failed: {status} | body: {body}"));
        }

        let payload: serde_json::Value = r.json().await?;
        if let Some(total) = payload.get("totalcount").and_then(|v| v.as_i64()) {
            log::info!("DEBUG: totalcount(status=New) = {}", total);
        }

        Self::parse_ticket_rows(
            payload.get("data").cloned().unwrap_or_default(),
            id_field,
            name_field,
            requester_field,
        )
    }

    /// Recent tickets (any status), useful for debug-list.
    pub async fn search_recent_tickets(
        &mut self,
        id_field: i64,
        name_field: i64,
        max_rows: usize,
    ) -> Result<Vec<Ticket>> {
        self.ensure_session().await?;

        let params: Vec<(&str, String)> = vec![
            ("sort", id_field.to_string()),
            ("order", "DESC".into()),
            ("range", format!("0-{}", max_rows)),
            ("forcedisplay[0]", id_field.to_string()),
            ("forcedisplay[1]", name_field.to_string()),
        ];

        let url = format!("{}/search/Ticket", self.base_url);
        let r = self.http.get(url).headers(self.hdrs()).query(&params).send().await?;
        if !r.status().is_success() {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            return Err(anyhow!("search/Ticket(recent) failed: {status} | body: {body}"));
        }
        let payload: serde_json::Value = r.json().await?;
        Self::parse_ticket_rows(
            payload.get("data").cloned().unwrap_or_default(),
            id_field,
            name_field,
            None,
        )
    }

    fn parse_ticket_rows(
        data: serde_json::Value,
        id_field: i64,
        name_field: i64,
        requester_field: Option<i64>,
    ) -> Result<Vec<Ticket>> {
        let mut out = Vec::new();
        let idk = id_field.to_string();
        let namek = name_field.to_string();
        let reqk = requester_field.map(|r| r.to_string());

        match data {
            serde_json::Value::Object(map) => {
                for (_, row) in map {
                    if let Some(t) = Self::row_to_ticket(&row, &idk, &namek, reqk.as_deref()) {
                        out.push(t);
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for row in arr {
                    if let Some(t) = Self::row_to_ticket(&row, &idk, &namek, reqk.as_deref()) {
                        out.push(t);
                    }
                }
            }
            _ => {}
        }
        Ok(out)
    }

    fn row_to_ticket(
        row: &serde_json::Value,
        idk: &str,
        namek: &str,
        reqk: Option<&str>,
    ) -> Option<Ticket> {
        use serde_json::Value;

        fn extract_i64(v: &Value) -> Option<i64> {
            match v {
                Value::String(s) => s.trim().parse::<i64>().ok(),
                Value::Number(n) => n.as_i64().or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok())),
                _ => None,
            }
        }

        fn extract_string(v: &Value) -> Option<String> {
            match v {
                Value::String(s) => Some(s.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
        }

        let id_v = row.get(idk)?;
        let id = extract_i64(id_v)?;
        let name = row.get(namek).and_then(extract_string).unwrap_or_default();
        let requester = reqk.and_then(|k| row.get(k)).and_then(extract_string);

        Some(Ticket { id, name, requester })
    }
}
