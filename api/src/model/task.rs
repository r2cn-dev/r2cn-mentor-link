use entity::{sea_orm_active_enums::TaskStatus, task};
use sea_orm::{ActiveValue::NotSet, Set};
use serde::{Deserialize, Serialize};

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewTask {
    pub owner: String,
    pub repo: String,
    pub github_issue_number: i32,
    pub github_repo_id: i64,
    pub github_issue_id: i64,
    pub score: i32,
    pub mentor_github_login: String,
    pub github_issue_title: String,
    pub github_issue_link: String,
}

impl From<NewTask> for task::ActiveModel {
    fn from(value: NewTask) -> Self {
        Self {
            id: NotSet,
            owner: Set(value.owner),
            repo: Set(value.repo),
            github_issue_number: Set(value.github_issue_number),
            github_repo_id: Set(value.github_repo_id),
            github_issue_id: Set(value.github_issue_id),
            score: Set(value.score),
            task_status: Set(TaskStatus::Open),
            finish_year: NotSet,
            finish_month: NotSet,
            mentor_github_login: Set(value.mentor_github_login),
            student_github_login: NotSet,
            student_name: NotSet,
            create_at: Set(chrono::Utc::now().naive_utc()),
            update_at: Set(chrono::Utc::now().naive_utc()),
            github_issue_title: Set(value.github_issue_title),
            github_issue_link: Set(value.github_issue_link),
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i32,
    pub owner: String,
    pub repo: String,
    pub github_issue_number: i32,
    pub github_repo_id: i64,
    pub github_issue_id: i64,
    pub score: i32,
    pub task_status: TaskStatus,
    pub student_github_login: Option<String>,
    pub mentor_github_login: String,
}

impl From<task::Model> for Task {
    fn from(value: task::Model) -> Self {
        Self {
            id: value.id,
            owner: value.owner,
            repo: value.repo,
            github_issue_number: value.github_issue_number,
            github_repo_id: value.github_repo_id,
            github_issue_id: value.github_issue_id,
            score: value.score,
            task_status: value.task_status,
            student_github_login: value.student_github_login,
            mentor_github_login: value.mentor_github_login,
        }
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchTask {
    pub github_repo_id: i64,
    pub github_mentor_login: String,
}

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandRequest {
    pub github_issue_id: i64,
    pub login: String,
    pub student_name: Option<String>,
}

#[derive(PartialEq, Eq, Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateScoreRequest {
    pub github_issue_id: i64,
    pub score: i32,
}