//! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.3

use super::sea_orm_active_enums::TaskStatus;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "task")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub github_repo_id: i64,
    #[sea_orm(unique)]
    pub github_issue_id: i64,
    pub score: i32,
    pub task_status: TaskStatus,
    pub student_github_login: Option<String>,
    pub mentor_github_login: String,
    pub create_at: DateTime,
    pub update_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
