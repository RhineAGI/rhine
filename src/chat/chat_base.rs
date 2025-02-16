use error_stack::{Context, Report, Result, ResultExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use spider::tokio_stream::StreamExt;
use thiserror::Error;
use tracing::debug;
use ureq::Error as UreqError;
use crate::config::{Config, ModelCapability, CFG};

#[derive(Debug, Error)]
pub enum ChatError {
    #[error("Failed to parse response")]
    ParseResponseError,
    #[error("Missing usage data")]
    MissingUsageData,
    #[error("HTTP error with status code: {0}")]
    HttpError(u16),
    #[error("Unknown error")]
    UnknownError,
}


// ---------- 基础聊天结构 ----------
#[derive(Debug, Clone)]
pub struct BaseChat {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub character_prompt: String,
    pub messages: Vec<Message>,
    pub usage: i32,
    pub need_stream: bool,
}

impl BaseChat {
    pub fn new_with_api_name(
        api_name: &str,
        character_prompt: &str,
        need_stream: bool,
    ) -> Self {
        let api_info = Config::get_api_info_with_name(api_name.to_string()).unwrap();

        Self {
            model: api_info.model,
            base_url: api_info.base_url,
            api_key: api_info.api_key,
            character_prompt: character_prompt.to_string(),
            messages: Vec::new(),
            usage: 0,
            need_stream,
        }
    }

    pub fn new_with_model_capability(
        model_capability: &ModelCapability,
        character_prompt: &str,
        need_stream: bool,
    ) -> Self {
        let api_info = Config::get_api_info_with_capablity(model_capability.clone()).unwrap();

        Self {
            model: api_info.model,
            base_url: api_info.base_url,
            api_key: api_info.api_key,
            character_prompt: character_prompt.to_string(),
            messages: Vec::new(),
            usage: 0,
            need_stream,
        }
    }

    pub fn add_message(&mut self, role: Role, content: &str) {
        self.messages.push(Message {
            role,
            content: content.to_string(),
        });
    }

    pub fn build_request_body(&self) -> serde_json::Value {
        let messages = self.build_messages();

        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": self.need_stream,
        });

        body
    }

    pub async fn send_request(
        &mut self,
        request_body: serde_json::Value,
    ) -> Result<serde_json::Value, ChatError> {
        let response = ureq::post(&self.base_url)
            .header("Content-Type", "application/json")
            .header("Authorization", &format!("Bearer {}", &self.api_key))
            .send_json(request_body.clone());

        match response {
            Ok(res) => {
                let parsed: serde_json::Value = res.into_body().read_json()
                    .change_context(ChatError::ParseResponseError)
                    .attach_printable("Failed to parse response JSON")?;

                self.usage += parsed["usage"]["total_tokens"]
                    .as_i64()
                    .ok_or_else(|| Report::new(ChatError::MissingUsageData))
                    .attach_printable("Missing usage data in response")?
                    as i32;

                Ok(parsed)
            }
            Err(UreqError::StatusCode(code)) => {
                Err(Report::new(ChatError::HttpError { 0: code })
                    .attach_printable(format!("HTTP Error: Status Code {} with request body {}", code, request_body)))
            }
            Err(_) => {
                Err(Report::new(ChatError::UnknownError)
                    .attach_printable(format!("Unknown Error occurred with request body: {}", request_body)))
            }

        }
    }

    // 私有方法：构建消息数组
    fn build_messages(&self) -> Vec<HashMap<String, String>> {
        let mut messages = vec![HashMap::from([
            ("role".to_owned(), "system".to_owned()),
            ("content".to_owned(), self.character_prompt.clone()),
        ])];

        messages.extend(
            self.messages
                .iter()
                .map(|m| m.to_api_format(&m.role))
                .collect::<Vec<_>>(),
        );

        messages
    }
}


// ---------- 数据结构 ----------
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    #[serde(untagged)]
    Character(String),
}

impl From<&str> for Role {
    fn from(s: &str) -> Self {
        match s {
            "system" => Self::System,
            "user" => Self::User,
            "assistant" => Self::Assistant,
            other => Self::Character(other.to_string()), // 关键转换！
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn to_api_format(&self, current_speaker: &Role) -> HashMap<String, String> {
        let (mut role_str, mut content) = match &self.role {
            Role::System => ("system", self.content.clone()),
            Role::User => ("user", self.content.clone()),
            Role::Assistant => ("assistant", self.content.clone()),
            Role::Character(c) => {
                // 判断是否是当前发言者
                if self.role == *current_speaker {
                    // 是发言者：作为assistant输出
                    ("assistant", self.content.clone())
                } else {
                    // 非发言者：添加前缀并作为user输出
                    let prefixed_content = format!("{} said: {}", c, self.content);
                    ("user", prefixed_content)
                }
            }
        };

        // 针对Assistant角色的特殊处理（可选）
        if let Role::Assistant = self.role {
            if self.role == *current_speaker {
                role_str = "assistant";
            } else {
                role_str = "user";
                content = format!("Assistant said: {}", self.content);
            }
        }

        HashMap::from([
            ("role".to_string(), role_str.to_string()),
            ("content".to_string(), content),
        ])
    }
}