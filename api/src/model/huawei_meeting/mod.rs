use entity::conference;
use sea_orm::{ActiveValue::NotSet, Set};
use serde::{Deserialize, Serialize};

pub mod app_auth;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Conferences {
    #[serde(rename = "conferenceID")]
    pub conference_id: String,
    pub subject: String,
    pub start_time: String,
    pub end_time: String,
    pub conference_state: String,
    pub language: String,
    pub record_type: i32,
    pub is_auto_record: i32,
    pub conf_type: String,
    pub chair_join_uri: String,
    pub guest_join_uri: String,
    pub scheduser_name: String,
}

impl From<Conferences> for conference::ActiveModel {
    fn from(value: Conferences) -> Self {
        conference::ActiveModel {
            id: NotSet,
            platform_type: Set("huaweimeeting".to_owned()),
            subject: Set(value.subject),
            start_time: Set(value.start_time),
            end_time: Set(value.end_time),
            conference_state: Set(value.conference_state),
            conference_id: Set(value.conference_id),
            language: Set(value.language),
            scheduser_name: Set(value.scheduser_name),
            record_type: Set(value.record_type),
            is_auto_record: Set(value.is_auto_record),
            conf_type: Set(value.conf_type),
            chair_join_uri: Set(value.chair_join_uri),
            guest_join_uri: Set(value.guest_join_uri),
            create_at: Set(chrono::Utc::now().naive_utc()),
            update_at: Set(chrono::Utc::now().naive_utc()),
        }
    }
}
