use std::env;

use lettre::message::{header, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use tera::{Context, Tera};

pub struct EmailSender {
    template_name: String,
    subject: String,
    context: Context,
    receiver: String,
}

impl EmailSender {
    pub fn new(template_name: &str, subject: &str, context: Context, receiver: &str) -> Self {
        EmailSender {
            template_name: template_name.to_owned(),
            subject: subject.to_owned(),
            context,
            receiver: receiver.to_owned(),
        }
    }

    pub fn send(&self) {
        let tera = Tera::new("templates/*").unwrap();
        let html_body = tera.render(&self.template_name, &self.context).unwrap();
        let email = Message::builder()
            .from("no-reply@r2cn.dev".parse().unwrap())
            .to(self.receiver.parse().unwrap())
            .subject(self.subject.clone())
            .singlepart(
                SinglePart::builder()
                    .header(header::ContentType::TEXT_HTML)
                    .body(html_body),
            )
            .unwrap();

        // 使用 AWS SES SMTP 服务器
        let creds = Credentials::new(
            env::var("AWS_SES_ACCOUNT").unwrap(),
            env::var("AWS_SES_PASSWORD").unwrap(),
        );

        let mailer = SmtpTransport::relay("email-smtp.ap-southeast-2.amazonaws.com") // AWS SES SMTP 地址
            .unwrap()
            .credentials(creds)
            .port(465)
            .build();

        match mailer.send(&email) {
            Ok(_) => println!("邮件发送成功"),
            Err(e) => eprintln!("邮件发送失败: {:?}", e),
        }
    }
}
