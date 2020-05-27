pub mod chat;
pub mod errors;
pub mod test;

use bytes::buf::BufExt as _;
use hyper::client::*;
use hyper::http::StatusCode;
use hyper::{Body, Request, Uri};
use hyper_tls::HttpsConnector;
use rsb_derive::Builder;
use std::io::Read;
use url::Url;

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone, Builder)]
pub struct SlackApiToken {
    value: String,
    workspace_id: Option<String>,
    scope: Option<String>,
}

#[derive(Debug)]
pub struct SlackClient {
    connector: Client<HttpsConnector<HttpConnector>>,
}

#[derive(Debug)]
pub struct SlackClientSession<'a> {
    client: &'a SlackClient,
    token: SlackApiToken,
}

pub type ClientResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct SlackEnvelopeMessage {
    ok: bool,
    error: Option<String>,
}

impl SlackClient {
    const SLACK_API_URI_STR: &'static str = "https://slack.com/api";

    fn create_method_uri_path(method_relative_uri: &str) -> String {
        format!("{}/{}", SlackClient::SLACK_API_URI_STR, method_relative_uri)
    }

    fn create_url(url_str: &String) -> Uri {
        url_str.parse().unwrap()
    }

    fn create_url_with_params<PT, TS>(url_str: &String, params: PT) -> Uri
    where
        PT: std::iter::IntoIterator<Item = (TS, Option<TS>)>,
        TS: std::string::ToString,
    {
        let url_query_params: Vec<(String, String)> = params
            .into_iter()
            .map(|(k, vo)| vo.map(|v| (k.to_string(), v.to_string())))
            .flatten()
            .collect();

        Url::parse_with_params(url_str.as_str(), url_query_params)
            .unwrap()
            .as_str()
            .parse()
            .unwrap()
    }

    pub fn new() -> Self {
        let https_connector = HttpsConnector::new();
        SlackClient {
            connector: Client::builder().build(https_connector),
        }
    }

    pub async fn send_webapi_request<RS>(&self, request: Request<Body>) -> ClientResult<RS>
    where
        RS: for<'de> serde::de::Deserialize<'de>,
    {
        let http_res = self.connector.request(request).await?;
        let http_status = http_res.status();
        let http_body = hyper::body::aggregate(http_res).await?;
        let mut http_reader = http_body.reader();
        let mut http_body_str = String::new();
        http_reader.read_to_string(&mut http_body_str)?;

        match http_status {
            StatusCode::OK => {
                let slack_message: SlackEnvelopeMessage =
                    serde_json::from_str(http_body_str.as_str())?;
                if slack_message.error.is_none() {
                    let decoded_body = serde_json::from_str(http_body_str.as_str())?;
                    Ok(decoded_body)
                } else {
                    Err(Box::new(errors::SlackClientError::ApiError(
                        errors::SlackClientApiError::new(slack_message.error.unwrap()),
                    )))
                }
            }
            _ => Err(Box::new(errors::SlackClientError::HttpError(
                errors::SlackClientHttpError::new(http_status)
                    .with_http_response_body(http_body_str),
            ))),
        }
    }

    pub fn open_session(&self, token: &SlackApiToken) -> SlackClientSession {
        SlackClientSession {
            client: &self,
            token: token.clone(),
        }
    }

    pub async fn get<RS, PT, TS>(&self, method_relative_uri: &str, params: PT) -> ClientResult<RS>
    where
        RS: for<'de> serde::de::Deserialize<'de>,
        PT: std::iter::IntoIterator<Item = (TS, Option<TS>)>,
        TS: std::string::ToString,
    {
        let full_uri = SlackClient::create_url_with_params(
            &SlackClient::create_method_uri_path(&method_relative_uri),
            params,
        );

        let body = self
            .send_webapi_request(Request::get(full_uri).body(Body::empty())?)
            .await?;

        Ok(body)
    }
}

impl<'a> SlackClientSession<'_> {
    fn setup_token_auth_header(
        &self,
        request_builder: hyper::http::request::Builder,
    ) -> hyper::http::request::Builder {
        let token_header_value = format!("Bearer {}", self.token.value);
        request_builder.header("Authorization", token_header_value)
    }

    pub async fn get<RS, PT, TS>(&self, method_relative_uri: &str, params: PT) -> ClientResult<RS>
    where
        RS: for<'de> serde::de::Deserialize<'de>,
        PT: std::iter::IntoIterator<Item = (TS, Option<TS>)>,
        TS: std::string::ToString,
    {
        let full_uri = SlackClient::create_url_with_params(
            &SlackClient::create_method_uri_path(&method_relative_uri),
            params,
        );

        let body = self
            .client
            .send_webapi_request(
                self.setup_token_auth_header(Request::get(full_uri))
                    .body(Body::empty())?,
            )
            .await?;

        Ok(body)
    }

    pub async fn post<RQ, RS, PT, TS>(
        &self,
        method_relative_uri: &str,
        request: RQ,
    ) -> ClientResult<RS>
    where
        RQ: serde::ser::Serialize,
        RS: for<'de> serde::de::Deserialize<'de>,
        PT: std::iter::IntoIterator<Item = (TS, Option<TS>)>,
        TS: std::string::ToString,
    {
        let full_uri =
            SlackClient::create_url(&SlackClient::create_method_uri_path(&method_relative_uri));

        let post_json = serde_json::to_string(&request)?;

        let response_body = self
            .client
            .send_webapi_request(
                self.setup_token_auth_header(Request::get(full_uri))
                    .header("content-type", "application/json")
                    .body(Body::from(post_json))?,
            )
            .await?;

        Ok(response_body)
    }
}