use error_stack::{Result, ResultExt, Report};
use serde::de::DeserializeOwned;
use thiserror::Error;
use tracing::log::info;
use crate::chat::chat_base::{BaseChat, ChatError, Role};
use crate::config::{ModelCapability, CFG};
use crate::config::ModelCapability::ToolUse;
use crate::schema::json_schema::JsonSchema;


pub struct ChatTool;

impl ChatTool {
    pub async fn get_json<T: DeserializeOwned + 'static + JsonSchema>(
        text_answer: &str,
        json_schema: serde_json::Value,
    ) -> Result<T, ChatError> {
        let mut base = BaseChat::new_with_model_capability(
            ToolUse,
            "将输入内容整理为指定的json形式输出",
            false,
        );

        base.add_message(Role::User, text_answer);

        let request_body = add_response_format(base.build_request_body(), json_schema);

        let response = base.get_response(request_body)
            .await
            .change_context(ChatError::GetJsonError)
            .attach_printable("Failed to send request")?;

        let json_answer = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or(Report::new(ChatError::GetJsonError))
            .attach_printable("Failed to get content from response")?;


        info!("Get LLM API Answer: {}", json_answer);

        // 添加助手回复
        base.add_message(Role::Assistant, json_answer);

        serde_json::from_str(json_answer)
            .change_context(ChatError::GetJsonError)
            .attach_printable_lazy(|| format!("Failed to deserialize JSON: {}", json_answer))
    }

    pub async fn get_function(
        text_answer: &str,
        tools_schema: serde_json::Value,
    ) -> Result<serde_json::Value, ChatError> {
        let mut base = BaseChat::new_with_model_capability(
            ToolUse,
            "根据输入的内容调用指定的函数",
            false,
        );

        base.add_message(Role::User, text_answer);

        let request_body = add_tools(base.build_request_body(), tools_schema);

        let response = base.get_response(request_body)
            .await
            .change_context(ChatError::GetFunctionError)
            .attach_printable("Failed to send request")?;

        let json_answer = response["choices"][0]["message"]["tool_calls"][0]["function"].clone();

        Ok(json_answer)
    }
}

fn add_response_format(
    mut request_body: serde_json::Value,
    schema: serde_json::Value,
) -> serde_json::Value {
    let response_format = serde_json::json!({
        "response_format": schema
    });

    if let serde_json::Value::Object(ref mut body) = request_body {
        if let serde_json::Value::Object(format) = response_format {
            body.extend(format);
        }
    }
    request_body
}

fn add_tools(mut request_body: serde_json::Value, schema: serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(ref mut body) = request_body {
        if let serde_json::Value::Object(format) = schema {
            body.extend(format);
        }
    }
    request_body
}