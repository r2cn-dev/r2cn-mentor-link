use std::sync::Arc;

use chrono::{Datelike, NaiveDate, Utc};
use entity::student;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, QueryFilter, Set,
};

use crate::ospp::ValidateStudentRes;
#[derive(Clone)]
pub struct StudentStorage {
    connection: Arc<DatabaseConnection>,
}

impl StudentStorage {
    pub fn get_connection(&self) -> &DatabaseConnection {
        &self.connection
    }

    pub async fn new(connection: Arc<DatabaseConnection>) -> Self {
        StudentStorage { connection }
    }

    pub async fn get_student_by_login(
        &self,
        login: &str,
    ) -> Result<Option<student::Model>, anyhow::Error> {
        let record = student::Entity::find()
            .filter(student::Column::GithubLogin.eq(login))
            .one(self.get_connection())
            .await?;
        Ok(record)
    }

    pub async fn insert_or_update_student(
        &self,
        login: &str,
        data: ValidateStudentRes,
    ) -> Result<(), anyhow::Error> {
        let contract_deadline = if let Some(contract_deadline) = data.contract_deadline {
            let contract_deadline =
                NaiveDate::parse_from_str(&contract_deadline, "%Y-%m-%d %H:%M:%S")
                    .expect("time formart not match");
            let contract_deadline = contract_deadline.with_day(1).unwrap();
            Some(contract_deadline)
        } else {
            None
        };

        let now = Utc::now().naive_utc();
        let student = self.get_student_by_login(login).await?;
        if let Some(student) = student {
            let mut a_model = student.into_active_model();
            a_model.contract_end_date = Set(contract_deadline);
            a_model.update_at = Set(now);
            a_model.update(self.get_connection()).await?;
        } else {
            let new_stu = student::ActiveModel {
                id: NotSet,
                github_login: Set(login.to_owned()),
                student_name: Set(data.student_name.unwrap()),
                contract_end_date: Set(contract_deadline),
                create_at: Set(now),
                update_at: Set(now),
                email: Set(data.email.unwrap()),
            };
            new_stu.insert(self.get_connection()).await?;
        }
        Ok(())
    }
}
