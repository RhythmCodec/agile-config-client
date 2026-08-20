//! HTTP pull of published application configuration.

use crate::auth::basic_authorization;
use crate::error::Error;
use crate::options::ClientOptions;
use crate::protocol::HEADER_KEY_PUBLISH_TIME_LINE_ID;

pub(crate) struct HttpPayload {
    pub json: String,
    pub publish_time_line_id: Option<String>,
}

pub(crate) fn config_url(node: &str, options: &ClientOptions) -> String {
    let app_id = urlencoding::encode(&options.app_id);
    let env = urlencoding::encode(&options.env);
    format!("{node}/api/config/app/{app_id}?env={env}")
}

pub(crate) async fn fetch_config(
    http: &reqwest::Client,
    node: &str,
    options: &ClientOptions,
) -> Result<HttpPayload, Error> {
    let url = config_url(node, options);
    let app_id_header = urlencoding::encode(&options.app_id);
    let response = http
        .get(&url)
        .header("appid", app_id_header.as_ref())
        .header(
            "Authorization",
            basic_authorization(&options.app_id, &options.secret),
        )
        .timeout(options.http_timeout)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        return Err(Error::HttpStatus {
            url,
            status: status.as_u16(),
        });
    }

    let publish_time_line_id = response
        .headers()
        .get(HEADER_KEY_PUBLISH_TIME_LINE_ID)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let json = response.text().await?;
    Ok(HttpPayload {
        json,
        publish_time_line_id,
    })
}

#[cfg(test)]
mod tests {
    use super::config_url;
    use crate::options::ClientOptions;

    #[test]
    fn config_url_encodes_app_id_and_env() {
        let options = ClientOptions {
            app_id: "my app".into(),
            env: "DEV".into(),
            ..ClientOptions::default()
        };
        assert_eq!(
            config_url("http://localhost:5000", &options),
            "http://localhost:5000/api/config/app/my%20app?env=DEV"
        );
    }
}
