use std::env;

use anyhow::Error;
use axum::{Router, extract::State, http::StatusCode, routing::post};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Datelike, Duration, Local, NaiveTime, Utc, Weekday};
use hmac::{Hmac, Mac};
use rand::{Rng, distr::Alphanumeric};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use sha2::Sha256;

use crate::model::huawei_meeting::{Conferences, app_auth::AppAuth};
use entity::conference;

use crate::AppState;

pub fn routers() -> Router<AppState> {
    Router::new().nest(
        "/conference",
        Router::new().route("/new", post(conference_create)),
    )
}

async fn conference_create(state: State<AppState>) -> Result<(), (StatusCode, &'static str)> {
    let client = reqwest::Client::new();
    let api_host = env::var("HUAWEI_MEETING_API_ENDPOINT").unwrap();
    let app_auth = account_auth().await.unwrap();

    let next_tuesday = next_tuesday_8pm();
    tracing::debug!("Next Tuesday at 8 PM is: {}", next_tuesday);
    let json_str = json!({
        "startTime": next_tuesday.as_str(),
        "mediaTypes": "HDVideo",
        "length": 60,
        "subject": "创建会议接口测试",
        "isAutoRecord": 1,
        "recordType": 2,
    });

    let res = client
        .post(format!("{}/v1/mmc/management/conferences", api_host))
        .header("X-Access-Token", app_auth.access_token)
        .json(&json_str)
        .send()
        .await
        .unwrap();

    let body = res.text().await.unwrap();
    match serde_json::from_str::<Vec<Conferences>>(&body) {
        Ok(conf) => {
            tracing::debug!("Create Meeting Return: {}", body);
            let a_model: conference::ActiveModel = conf.first().unwrap().to_owned().into();
            let conf_stg = state.context.conf_stg();
            conf_stg.save_conf(a_model).await.unwrap();
        }
        Err(err) => {
            tracing::error!("parsing err:{}", err);
            tracing::error!("huaweimeeting api return:{}", body);
        }
    }
    Ok(())
}

#[allow(dead_code)]
async fn app_auth() -> Result<AppAuth, Error> {
    let ten_minutes_later = Utc::now() + Duration::minutes(10);
    let expire_time = ten_minutes_later.timestamp();
    let nonce = generate_random_string();

    let app_id =
        env::var("HUAWEI_MEETING_APP_ID").expect("HUAWEI_MEETING_APP_ID is not set in .env file");
    let app_key =
        env::var("HUAWEI_MEETING_APP_KEY").expect("HUAWEI_MEETING_APP_KEY is not set in .env file");
    tracing::warn!("nonce is: {}, expire_time is: {}", nonce, expire_time);

    let user_id = "afc560f67c484ce785818078adee6193";
    let data = format!("{}:{}:{}:{}", app_id, user_id, expire_time, nonce);
    tracing::info!("data:{}", data);
    let signature = calculate_hmac_sha256(app_key.as_bytes(), data.as_bytes());

    let json_str = json!({
        "appId": app_id.clone(),
        "clientType": 72,
        "expireTime": expire_time,
        "nonce": nonce,
        // "userEmail": user_id,
        "userId": user_id,
        // "userName": "y****g",
        // "userPhone": "156****6750",
    });

    let hmac_sha256 = format!(
        "HMAC-SHA256 signature={},access={}",
        signature,
        STANDARD.encode(app_id)
    );
    tracing::info!("HMAC-SHA256 Signature: {}", hmac_sha256);

    let client = reqwest::Client::new();
    let api_host = env::var("HUAWEI_MEETING_API_ENDPOINT").unwrap();
    let res = client
        .post(format!("{}/v2/usg/acs/auth/appauth", api_host))
        .header(AUTHORIZATION, hmac_sha256)
        .header(CONTENT_TYPE, "application/json;charset=UTF-8")
        .json(&json_str)
        .send()
        .await
        .unwrap();

    let body = res.text().await.unwrap();
    match serde_json::from_str::<AppAuth>(&body) {
        Ok(app_auth) => Ok(app_auth),
        Err(err) => {
            tracing::error!("parsing err:{}", err);
            tracing::error!("huaweimeetng api return:{}", body);
            Err(err.into())
        }
    }
}

async fn account_auth() -> Result<AppAuth, Error> {
    let account = "p-afc560f67c484ce785818078adee6193";
    let password = "tetvyn-jazDa9-mykqek";

    let json_str = json!({
        "clientType": 72,
        "account": account,
    });

    let client = reqwest::Client::new();
    let api_host = env::var("HUAWEI_MEETING_API_ENDPOINT").unwrap();
    let res = client
        .post(format!("{}/v1/usg/acs/auth/account", api_host))
        .header(
            AUTHORIZATION,
            format!(
                "Basic {}",
                STANDARD.encode(format!("{}:{}", account, password))
            ),
        )
        .header(CONTENT_TYPE, "application/json;charset=UTF-8")
        .json(&json_str)
        .send()
        .await
        .unwrap();

    let body = res.text().await.unwrap();
    match serde_json::from_str::<AppAuth>(&body) {
        Ok(app_auth) => Ok(app_auth),
        Err(err) => {
            tracing::error!("parsing err:{}", err);
            tracing::error!("huaweimeetng api return:{}", body);
            Err(err.into())
        }
    }
}

#[allow(dead_code)]
fn calculate_hmac_sha256(key: &[u8], data: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("Invalid HMAC key length");

    mac.update(data);

    let result = mac.finalize();
    let code_bytes = result.into_bytes();
    hex::encode(code_bytes)
}

#[allow(dead_code)]
fn generate_random_string() -> String {
    let random_string: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    random_string
}

fn next_tuesday_8pm() -> String {
    let now: chrono::DateTime<Local> = Local::now();

    let target_time = NaiveTime::from_hms_opt(20, 0, 0).unwrap();

    let days_to_add = match now.weekday() {
        Weekday::Tue if now.time() < target_time => 0,
        Weekday::Tue => 7,
        current_weekday => {
            (Weekday::Tue.num_days_from_monday() + 7 - current_weekday.num_days_from_monday()) % 7
        }
    };
    let next_tuesday_date = now + Duration::days(days_to_add as i64);
    next_tuesday_date
        .date_naive()
        .and_time(target_time)
        .and_utc()
        .format("%Y-%m-%d %H:%M")
        .to_string()
}
