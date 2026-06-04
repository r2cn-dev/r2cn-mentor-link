use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    vec,
};

use anyhow::{Context, Error};
use axum::extract::State;
use chrono::{Datelike, NaiveDate};
use entity::{opensource_notification_preference, sea_orm_active_enums::TaskStatus, task};
use lettre::{
    Message, SmtpTransport, Transport,
    message::{Attachment, Body, MultiPart, SinglePart, header},
    transport::smtp::authentication::Credentials,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use service::{model::score::ScoreDto, storage::student_stg::StudentProfile};
use tera::Tera;

use crate::AppState;

enum Lang {
    Zh,
    En,
}

#[derive(Copy, Clone)]
enum NotificationKind {
    MonthlyPointsReport,
    TaskApplicationNotice,
    TaskApplicationSuccess,
    TaskCompletionReport,
    MentorTaskCreated,
    TaskFailed,
    SystemAnnouncement,
}

fn is_preference_enabled(
    preference: &opensource_notification_preference::Model,
    kind: NotificationKind,
) -> bool {
    match kind {
        NotificationKind::MonthlyPointsReport => preference.monthly_points_report,
        NotificationKind::TaskApplicationNotice => preference.task_application_notice,
        NotificationKind::TaskApplicationSuccess => preference.task_application_success,
        NotificationKind::TaskCompletionReport => preference.task_completion_report,
        NotificationKind::MentorTaskCreated => preference.mentor_task_created,
        NotificationKind::TaskFailed => preference.task_failed,
        NotificationKind::SystemAnnouncement => preference.system_announcement,
    }
}

async fn is_notification_enabled(state: &AppState, user_id: &str, kind: NotificationKind) -> bool {
    if !state.email_enabled() {
        return false;
    }

    match opensource_notification_preference::Entity::find()
        .filter(opensource_notification_preference::Column::UserId.eq(user_id))
        .one(state.context.services.student_stg.get_connection())
        .await
    {
        Ok(Some(preference)) => is_preference_enabled(&preference, kind),
        Ok(None) => true,
        Err(err) => {
            tracing::warn!(
                "Failed to read notification preference for {}: {}",
                user_id,
                err
            );
            true
        }
    }
}

/// 默认发件人地址，可通过环境变量 EMAIL_FROM 配置
fn from_email() -> String {
    env::var("EMAIL_FROM").unwrap_or_else(|_| "no-reply@r2cn.dev".to_string())
}

fn month_name(date: NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::En => date.format("%b").to_string(),
        Lang::Zh => format!("{}月", date.month()),
    }
}

// 用 mrml 将 MJML 转换为 HTML
pub fn render_mjml(template_name: &str, context: &tera::Context) -> Result<String, anyhow::Error> {
    let mut base = PathBuf::from(std::env::var("TEMPLATE_DIR").expect("TEMPLATE_DIR not set"));
    base.push("templates/mjml/*");
    let tera = Tera::new(base.to_str().unwrap()).context("Tera 初始化失败")?;
    let mjml_content = tera
        .render(template_name, context)
        .context("Tera 渲染失败")?;

    let mjml_content = mjml_content.replace("\r\n", "\n");

    let root = mrml::parse(&mjml_content).context("MJML 解析失败")?;
    let opts = mrml::prelude::render::RenderOptions::default();
    let html = root.render(&opts).context("MJML 渲染失败")?;
    Ok(html)
}

/// 创建带有 Content-ID 的内嵌图片附件
pub fn create_cid_attachment(image_path: &str, cid: &str) -> Result<SinglePart, anyhow::Error> {
    let mut base = PathBuf::from(std::env::var("TEMPLATE_DIR").expect("TEMPLATE_DIR not set"));
    base.push(image_path);
    let image_data = fs::read(Path::new(&base)).context("读取图片失败")?;

    let body = Body::new(image_data);
    let content_type_header: header::ContentType = "image/png".parse().unwrap();
    Ok(Attachment::new_inline(cid.to_string()).body(body, content_type_header))
}

/// 根据模板名称获取需要内嵌的图片
pub fn cid_images_for_template(template_name: &str) -> Vec<(&'static str, &'static str)> {
    let mut imgs: Vec<(&'static str, &'static str)> =
        vec![("templates/image/background.png", "background")];

    match template_name {
        "task_assigned.mjml" => {
            imgs.push(("templates/image/task_assigned.png", "task_status"));
        }
        "task_created_mentor.mjml" => {
            imgs.push(("templates/image/task_assigned.png", "task_status"));
        }
        "task_apply_mentor.mjml" => {
            imgs.push(("templates/image/task_assigned.png", "task_status"));
        }
        "task_failed.mjml" => {
            imgs.push(("templates/image/task_failed.png", "task_status"));
        }
        "task_completed_points.mjml" => {
            imgs.push(("templates/image/task_completed.png", "task_status"));
        }
        "monthly_points_summary.mjml" => {
            imgs.push(("templates/image/task_points.png", "task_status"));
        }
        _ => {}
    }

    imgs
}

pub enum EmailContent {
    /// local HTML template
    LocalTemplate {
        template_name: String,
        subject: String,
        context: tera::Context,
    },

    /// ZeptoMail template
    ZeptoTemplate {
        template_id: String,
        variables: serde_json::Value,
    },
}

pub struct EmailSender {
    content: EmailContent,
    receivers: Vec<String>,
    cc_email: Vec<String>,
}

impl EmailSender {
    pub fn from_local_template(
        template_name: &str,
        subject: &str,
        context: tera::Context,
        receiver: &str,
        cc_email: Vec<String>,
    ) -> Self {
        Self {
            receivers: vec![receiver.to_string()],
            cc_email,
            content: EmailContent::LocalTemplate {
                template_name: template_name.to_string(),
                subject: subject.to_string(),
                context,
            },
        }
    }

    pub fn from_zeptomail_template(
        template_id: &str,
        variables: serde_json::Value,
        receivers: Vec<String>,
        cc_email: Vec<String>,
    ) -> Self {
        Self {
            receivers,
            cc_email,
            content: EmailContent::ZeptoTemplate {
                template_id: template_id.to_string(),
                variables,
            },
        }
    }

    pub async fn send(&self) -> Result<(), Error> {
        match &self.content {
            EmailContent::LocalTemplate {
                template_name,
                subject,
                context,
            } => self.send_local(template_name, subject, context).await,

            EmailContent::ZeptoTemplate {
                template_id,
                variables,
            } => self.send_zeptomail(template_id, variables).await,
        }
    }

    async fn send_local(
        &self,
        template_name: &str,
        subject: &str,
        context: &tera::Context,
    ) -> Result<(), Error> {
        let html_body = if template_name.ends_with(".mjml") {
            render_mjml(template_name, context)
        } else {
            let mut base =
                PathBuf::from(std::env::var("TEMPLATE_DIR").expect("TEMPLATE_DIR not set"));
            base.push("templates/*");
            Tera::new(base.to_str().unwrap())
                .context("Tera 初始化失败")
                .and_then(|t| t.render(template_name, context).context("Tera 渲染失败: "))
        }?;

        let html_part = SinglePart::builder()
            .header(header::ContentType::TEXT_HTML)
            .body(html_body);

        let mut email_builder = Message::builder()
            .from(from_email().parse().unwrap())
            .to(self.receivers[0].parse().unwrap())
            .subject(subject);

        for cc_addr in &self.cc_email {
            match cc_addr.parse() {
                Ok(mailbox) => {
                    email_builder = email_builder.cc(mailbox);
                }
                Err(e) => {
                    tracing::warn!("无效的 CC 邮箱 {}: {}", cc_addr, e);
                }
            }
        }

        let email = if template_name.ends_with(".mjml") {
            let mut multipart = MultiPart::related().singlepart(html_part);
            for (img_path, cid) in cid_images_for_template(template_name) {
                match create_cid_attachment(img_path, cid) {
                    Ok(part) => {
                        multipart = multipart.singlepart(part);
                    }
                    Err(err) => tracing::warn!("内嵌图片加载失败 {}: {}", img_path, err),
                }
            }
            email_builder.multipart(multipart)
        } else {
            email_builder.singlepart(html_part)
        }?;

        let creds = Credentials::new(env::var("ZEPTO_AK").unwrap(), env::var("ZEPTO_SK").unwrap());

        let mailer = SmtpTransport::starttls_relay("smtp.zeptomail.com")
            .unwrap()
            .credentials(creds)
            .build();

        match mailer.send(&email) {
            Ok(_) => tracing::info!("邮件发送成功: to {} ", self.receivers[0]),
            Err(e) => tracing::error!("邮件发送失败: {:?}, to {}", e, self.receivers[0]),
        }

        Ok(())
    }

    async fn send_zeptomail(
        &self,
        template_id: &str,
        _variables: &serde_json::Value,
    ) -> Result<(), Error> {
        let to_list: Vec<_> = self
            .receivers
            .iter()
            .map(|email| {
                json!({
                    "email_address": {
                        "address": email
                    }
                })
            })
            .collect();

        let body = json!({
            "template_key": template_id,
            "from": { "address": from_email() },
            "to": to_list
        });

        let resp = reqwest::Client::new()
            .post("https://api.zeptomail.com/v1.1/email/template/batch")
            .header(
                "Authorization",
                format!("Zoho-enczapikey {}", env::var("ZEPTO_SK").unwrap()),
            )
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let body = resp.text().await?;
            tracing::error!("sending result: {}", body);
            Err(anyhow::Error::msg(body))
        }
    }

    pub async fn notice_all_email(state: State<AppState>, template_id: &str) -> Result<(), Error> {
        if !state.email_enabled() {
            tracing::info!("Email sending disabled by SEND_EMAIL, skip notice_all_email");
            return Ok(());
        }
        let active_mentors = state.mentor_stg().get_approved_mentors().await.unwrap();
        let mut active_mentor_emails = Vec::new();
        for mentor in active_mentors {
            if is_notification_enabled(
                &state,
                &mentor.user_id,
                NotificationKind::SystemAnnouncement,
            )
            .await
            {
                active_mentor_emails.push(mentor.email);
            }
        }

        let active_students = state.student_stg().get_active_students().await.unwrap();
        let mut active_student_emails = Vec::new();
        for student in active_students {
            if is_notification_enabled(
                &state,
                &student.user_id,
                NotificationKind::SystemAnnouncement,
            )
            .await
            {
                active_student_emails.push(student.email);
            }
        }
        let mut sending_emails = HashSet::new();
        sending_emails.extend(active_mentor_emails);
        sending_emails.extend(active_student_emails);
        tracing::info!("sending emails {:?}", sending_emails);

        let sender = EmailSender::from_zeptomail_template(
            template_id,
            serde_json::Value::Null,
            sending_emails.into_iter().collect(),
            vec![],
        );
        sender.send().await
    }

    pub async fn failed_email(state: State<AppState>, task: task::Model) {
        if let Some(student_id) = &task.student_id {
            let student = state
                .student_stg()
                .get_student_by_student_id(student_id)
                .await
                .unwrap();

            let mentor_login = &task.mentor_login;
            let mentor = state
                .mentor_stg()
                .get_mentor_by_login(mentor_login)
                .await
                .unwrap();
            let mentor_name = mentor
                .as_ref()
                .map(|m| m.login.clone())
                .unwrap_or_else(|| mentor_login.to_string());
            let mut cc_email = Vec::new();
            if let Some(mentor) = mentor
                && is_notification_enabled(&state, &mentor.user_id, NotificationKind::TaskFailed)
                    .await
            {
                cc_email.push(mentor.email);
            }

            if let Some(student) = student {
                if !is_notification_enabled(&state, &student.user_id, NotificationKind::TaskFailed)
                    .await
                {
                    tracing::info!("Skip failed_email by user preference: {}", student.user_id);
                    return;
                }
                let mut email_context = tera::Context::new();
                email_context.insert("student_name", &student.student_name);
                email_context.insert("task_title", &task.issue_title);
                email_context.insert("task_link", &task.issue_link);
                email_context.insert("mentor_name", &mentor_name);
                email_context.insert("project_link", &util::project_link(&task));

                let sender = EmailSender::from_local_template(
                    "task_failed.mjml",
                    "R2CN任务失败通知/R2CN Task Failure",
                    email_context,
                    &student.email,
                    cc_email,
                );
                sender.send().await.unwrap();
            }
        }
    }

    pub async fn assigned_email(state: State<AppState>, task: task::Model) {
        if let Some(student_id) = &task.student_id {
            let student = state
                .student_stg()
                .get_student_by_student_id(student_id)
                .await
                .unwrap()
                .unwrap();

            let mentor_login = &task.mentor_login;
            let mentor = state
                .mentor_stg()
                .get_mentor_by_login(mentor_login)
                .await
                .unwrap();
            let mentor_name = mentor
                .as_ref()
                .map(|m| m.login.clone())
                .unwrap_or_else(|| mentor_login.to_string());
            let mut cc_email = Vec::new();
            if let Some(mentor) = mentor
                && is_notification_enabled(
                    &state,
                    &mentor.user_id,
                    NotificationKind::TaskApplicationSuccess,
                )
                .await
            {
                cc_email.push(mentor.email);
            }

            if !is_notification_enabled(
                &state,
                &student.user_id,
                NotificationKind::TaskApplicationSuccess,
            )
            .await
            {
                tracing::info!(
                    "Skip assigned_email by user preference: {}",
                    student.user_id
                );
                return;
            }
            let mut email_context = tera::Context::new();
            email_context.insert("student_name", &student.student_name);
            email_context.insert("task_title", &task.issue_title);
            email_context.insert("task_link", &task.issue_link);
            email_context.insert("mentor_name", &mentor_name);
            email_context.insert("project_link", &util::project_link(&task));
            let sender = EmailSender::from_local_template(
                "task_assigned.mjml",
                "R2CN任务认领通知/R2CN Task Assigned",
                email_context,
                &student.email,
                cc_email,
            );
            sender.send().await.unwrap();
        }
    }

    pub async fn complete_email(state: State<AppState>, task: task::Model, balance: i32) {
        if let Some(student_id) = &task.student_id {
            let student = state
                .student_stg()
                .get_student_by_student_id(student_id)
                .await
                .unwrap();

            let mentor_login = &task.mentor_login;
            let mentor = state
                .mentor_stg()
                .get_mentor_by_login(mentor_login)
                .await
                .unwrap();
            let mentor_name = mentor
                .as_ref()
                .map(|m| m.login.clone())
                .unwrap_or_else(|| mentor_login.to_string());
            let mut cc_email = Vec::new();
            if let Some(mentor) = mentor
                && is_notification_enabled(
                    &state,
                    &mentor.user_id,
                    NotificationKind::TaskCompletionReport,
                )
                .await
            {
                cc_email.push(mentor.email);
            }

            if let Some(student) = student {
                if !is_notification_enabled(
                    &state,
                    &student.user_id,
                    NotificationKind::TaskCompletionReport,
                )
                .await
                {
                    tracing::info!(
                        "Skip complete_email by user preference: {}",
                        student.user_id
                    );
                    return;
                }
                let mut email_context = tera::Context::new();
                email_context.insert("student_name", &student.student_name);
                email_context.insert("task_title", &task.issue_title);
                email_context.insert("task_link", &task.issue_link);
                email_context.insert("mentor_name", &mentor_name);
                email_context.insert("points_total", &balance);
                email_context.insert("project_link", &util::project_link(&task));
                let sender = EmailSender::from_local_template(
                    "task_completed_points.mjml",
                    "R2CN任务完成通知/R2CN Task Successful",
                    email_context,
                    &student.email,
                    cc_email,
                );
                sender.send().await.unwrap();
            }
        }
    }

    pub async fn monthly_score_email(
        state: State<AppState>,
        student: Option<StudentProfile>,
        last_month: ScoreDto,
    ) {
        if let Some(student) = student {
            if !is_notification_enabled(
                &state,
                &student.user_id,
                NotificationKind::MonthlyPointsReport,
            )
            .await
            {
                tracing::info!(
                    "Skip monthly_score_email by user preference: {}",
                    student.user_id
                );
                return;
            }
            let mut email_context = tera::Context::new();
            email_context.insert("student_name", &student.student_name);
            email_context.insert("points_earned_month", &last_month.new_score);
            email_context.insert("points_redeemed_month", &last_month.consumption_score);
            email_context.insert("points_balance", &last_month.score_balance());

            let finished_tasks_last_month = state
                .task_stg()
                .get_student_tasks_with_status_in_month(
                    &student.student_id,
                    TaskStatus::finish_task_status(),
                    last_month.year,
                    last_month.month,
                )
                .await
                .unwrap();

            let mentor_logins = finished_tasks_last_month
                .iter()
                .map(|t| t.mentor_login.clone())
                .filter(|login| !login.is_empty())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            let mentors = state
                .mentor_stg()
                .get_mentors_by_logins(mentor_logins)
                .await
                .unwrap();

            let mut active_mentor_emails = Vec::new();
            for mentor in mentors {
                if is_notification_enabled(
                    &state,
                    &mentor.user_id,
                    NotificationKind::MonthlyPointsReport,
                )
                .await
                {
                    active_mentor_emails.push(mentor.email);
                }
            }

            let date =
                NaiveDate::from_ymd_opt(last_month.year, last_month.month as u32, 1).unwrap();

            let subject = format!(
                "R2CN{}积分报告/R2CN Monthly Score Report - {}.",
                month_name(date, Lang::Zh),
                month_name(date, Lang::En)
            );
            let sender = EmailSender::from_local_template(
                "monthly_points_summary.mjml",
                &subject,
                email_context,
                &student.email,
                active_mentor_emails,
            );
            sender.send().await.unwrap();
        }
    }

    pub async fn task_created_mentor_email(state: State<AppState>, task: task::Model) {
        let mentor = state
            .mentor_stg()
            .get_mentor_by_login(&task.mentor_login)
            .await
            .unwrap();

        if let Some(mentor) = mentor {
            if !is_notification_enabled(
                &state,
                &mentor.user_id,
                NotificationKind::MentorTaskCreated,
            )
            .await
            {
                tracing::info!(
                    "Skip task_created_mentor_email by user preference: {}",
                    mentor.user_id
                );
                return;
            }
            let mut email_context = tera::Context::new();
            email_context.insert("mentor_name", &mentor.name);
            email_context.insert("task_title", &task.issue_title);
            email_context.insert("task_link", &task.issue_link);
            email_context.insert("project_link", &util::project_link(&task));
            let sender = EmailSender::from_local_template(
                "task_created_mentor.mjml",
                "R2CN任务创建成功通知/R2CN Task Created",
                email_context,
                &mentor.email,
                vec![],
            );
            sender.send().await.unwrap();
        }
    }

    pub async fn task_apply_mentor_email(
        state: State<AppState>,
        task: task::Model,
        student_login: String,
    ) {
        let mentor = state
            .mentor_stg()
            .get_mentor_by_login(&task.mentor_login)
            .await
            .unwrap();

        if let Some(mentor) = mentor {
            if !is_notification_enabled(
                &state,
                &mentor.user_id,
                NotificationKind::TaskApplicationNotice,
            )
            .await
            {
                tracing::info!(
                    "Skip task_apply_mentor_email by user preference: {}",
                    mentor.user_id
                );
                return;
            }
            let mut email_context = tera::Context::new();
            email_context.insert("mentor_name", &mentor.name);
            email_context.insert("student_login", &student_login);
            email_context.insert("task_title", &task.issue_title);
            email_context.insert("task_link", &task.issue_link);
            email_context.insert("project_link", &util::project_link(&task));
            let sender = EmailSender::from_local_template(
                "task_apply_mentor.mjml",
                "R2CN学生申请任务通知/R2CN Student Application",
                email_context,
                &mentor.email,
                vec![],
            );
            sender.send().await.unwrap();
        }
    }
}

pub mod util {

    use entity::task::{self};

    pub fn project_link(task: &task::Model) -> String {
        format!("https://github.com/{}/{}", task.owner, task.repo)
    }
}

#[cfg(test)]
mod test {
    use std::env;

    use lettre::{
        Message, SmtpTransport, Transport,
        message::{MultiPart, SinglePart, header},
        transport::smtp::authentication::Credentials,
    };

    use super::{EmailSender, cid_images_for_template, create_cid_attachment, render_mjml};
    use crate::email::EmailContent;

    #[test]
    pub fn test_local_temp_email() {
        dotenvy::dotenv().ok();

        let mut email_context = tera::Context::new();
        email_context.insert("student_name", "name");
        email_context.insert("task_title", "title");
        email_context.insert(
            "task_link",
            "https://github.com/benjamin-747/r2cn-bot-test/issues/22",
        );
        email_context.insert(
            "project_link",
            "https://github.com/benjamin-747/r2cn-bot-test",
        );
        email_context.insert("task_id", "123");
        email_context.insert("mentor_name", "name");
        email_context.insert("mentor_email", "email");
        email_context.insert("points_earned", "25");
        email_context.insert("points_total", "100");
        email_context.insert("failure_reason", "fail reason");
        email_context.insert("review_comments", "comments");
        email_context.insert("resubmit_link", "");
        email_context.insert("report_month", "12");
        email_context.insert("generated_at", "20251212");
        email_context.insert("points_earned_month", "60");
        email_context.insert("points_redeemed_month", "100");
        email_context.insert("points_balance", "15");

        let sender = EmailSender::from_local_template(
            "task_assigned.mjml",
            "R2CN任务完成",
            email_context,
            "yetianxing2014@gmail.com",
            vec![("yetianxing2014@gmail.com".to_owned())],
        );

        match sender.content {
            EmailContent::LocalTemplate {
                template_name,
                subject,
                context,
            } => {
                let html_body = render_mjml(&template_name, &context).unwrap();
                let mut multipart = MultiPart::related().singlepart(
                    SinglePart::builder()
                        .header(header::ContentType::TEXT_HTML)
                        .body(html_body),
                );
                for (img_path, cid) in cid_images_for_template(&template_name) {
                    let part = create_cid_attachment(img_path, cid).unwrap();
                    multipart = multipart.singlepart(part);
                }
                let email = Message::builder()
                    .from("test-send@r2cn.dev".parse().unwrap())
                    .to(sender.receivers[0].parse().unwrap())
                    .subject(subject.clone())
                    .multipart(multipart)
                    .unwrap();

                let creds =
                    Credentials::new(env::var("ZEPTO_AK").unwrap(), env::var("ZEPTO_SK").unwrap());

                let mailer = SmtpTransport::starttls_relay("smtp.zeptomail.com")
                    .unwrap()
                    .credentials(creds)
                    .build();

                match mailer.send(&email) {
                    Ok(_) => println!("邮件发送成功"),
                    Err(e) => eprintln!("邮件发送失败: {:?}", e),
                }
            }
            EmailContent::ZeptoTemplate {
                template_id: _,
                variables: _,
            } => {
                todo!()
            }
        }
    }

    #[tokio::test]
    pub async fn test_online_temp_email() {
        dotenvy::dotenv().ok();

        let sender = EmailSender::from_zeptomail_template(
            "2d6f.48ef870ad8d4d3f2.k1.e5f70d71-1db7-11f1-ac5e-fae9afc80e45.19cdfca67c5",
            serde_json::Value::Null,
            vec![
                "yetianxing2014@gmail.com".to_string(),
                "715804430@qq.com".to_string(),
            ],
            vec![],
        );
        sender.send().await.unwrap();
    }
}
