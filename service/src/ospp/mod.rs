use std::env;

use common::errors::CommonError;
use serde::{Deserialize, Serialize};
use serde_json::Error;

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsppValidateStudentRes {
    pub code: i32,
    pub err_code: i32,
    #[serde(alias = "studentExist")]
    pub student_exist: bool,
    pub message: String,
    #[serde(alias = "suStudentName")]
    pub su_student_name: Option<String>,
    #[serde(alias = "contractDeadline")]
    pub contract_deadline: Option<String>,
    pub email: Option<String>,
}

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidateStudent {
    pub login: String,
}

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidateStudentRes {
    pub success: bool,
    pub student_name: Option<String>,
    pub contract_deadline: Option<String>,
    pub email: Option<String>,
}

pub async fn validate_student(json: ValidateStudent) -> Result<ValidateStudentRes, CommonError> {
    //call ospp api check status
    let client = reqwest::Client::new();
    let api_host = env::var("OSPP_API_ENDPOINT").unwrap();
    let res = client
        .get(format!("{}/api/r2cnStudent/{}", api_host, json.login))
        .send()
        .await
        .unwrap();
    let body = res.text().await.unwrap();
    tracing::debug!("ospp api response body:{:?}", body);

    // 解析 JSON
    let data: Result<OsppValidateStudentRes, Error> = serde_json::from_str(&body);
    match data {
        Ok(data) => Ok(ValidateStudentRes {
            success: data.student_exist,
            student_name: data.su_student_name,
            contract_deadline: data.contract_deadline,
            email: data.email,
        }),
        Err(err) => {
            tracing::error!("JSON parse error: {}", err);
            Ok(ValidateStudentRes {
                success: false,
                student_name: None,
                contract_deadline: None,
                email: None,
            })
        }
    }
}
